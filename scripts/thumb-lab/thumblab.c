// thumblab: one in-process candidate of the thumbnail lab (P12 block 6). Uses libav directly, one long-lived demuxer
// and decoder per worker, and reports one JSON line. lab.py runs it over the media and options.
//
//   thumblab FILE [--count N] [--width W] [--mode key|exact|sweep] [--order rev|sorted|levels] [--workers N]
//            [--threads N] [--skip none|nonref|bidir|nokey] [--loopfilter 0|1] [--scaler fast|bilinear|bicubic]
//            [--discard 0|1] [--lowres N] [--drain 0|1] [--lowdelay 0|1] [--seconds S] [--dump DIR]
//
// --drain 1 hands the decoder the keyframe and drains it at once. Without it a decoder with a reorder delay (H.264, HEVC)
// holds the frame back until the next frames arrive, and with skip_frame=nokey that means reading a whole GOP per seek.
// --seconds S stops after S seconds (the load test wants thumbnails per second under contention).
//
// mode key:   seek backward to the slot's time, decode only the keyframe found there.
// mode exact: seek backward, decode up to the slot's time.
// mode sweep: read the file once from the start, keep the frame nearest to each slot (order is ignored).
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/imgutils.h>
#include <libswscale/swscale.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <time.h>

typedef struct {
    const char *path;
    int count, width, exact, sweep, workers, threads, loopfilter, discard_demux, lowres, drain, lowdelay;
    double seconds;
    enum AVDiscard skip;
    int sws_flags;
    const char *dump;
    double duration, t0;
    int *order;
    double *arrival;  // seconds since t0, by arrival rank
    double *got_time; // the shown frame's time, by slot (-1 = none)
    atomic_int next, arrived;
    double max_ms;
    pthread_mutex_t lock;
} Lab;

static double now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec / 1e9;
}

static double slot_time(const Lab *l, int slot) { return (slot + 0.5) * l->duration / l->count; }

static int cmp_int(const void *a, const void *b) { return *(const int *)a - *(const int *)b; }

// Bit-reversal: every prefix of the order covers the grid evenly (van der Corput).
static void order_rev(int *o, int n) {
    int bits = 0, k = 0;
    while ((1 << bits) < n) bits++;
    for (unsigned i = 0; i < (1u << bits); i++) {
        unsigned r = 0;
        for (int b = 0; b < bits; b++) r |= ((i >> b) & 1) << (bits - 1 - b);
        if ((int)r < n) o[k++] = (int)r;
    }
}

// Levels: 5 evenly spread, then halve the gaps again and again; every level runs left to right (kind to hard disks).
static void order_levels(int *o, int n) {
    char *seen = calloc((size_t)n, 1);
    int k = 0;
    for (int parts = 5; k < n; parts *= 2) {
        int start = k;
        for (int i = 0; i < parts; i++) {
            int s = (int)((i + 0.5) * n / parts);
            if (s < n && !seen[s]) {
                seen[s] = 1;
                o[k++] = s;
            }
        }
        qsort(o + start, (size_t)(k - start), sizeof(int), cmp_int);
        if (parts > 4 * n) break;
    }
    for (int s = 0; s < n && k < n; s++)
        if (!seen[s]) o[k++] = s;
    free(seen);
}

typedef struct {
    AVFormatContext *f;
    AVCodecContext *d;
    int si;
    AVStream *st;
    struct SwsContext *sws;
    uint8_t *dst[4];
    int ls[4], height;
    AVPacket *pkt;
    AVFrame *fr;
} Worker;

static int worker_open(Lab *l, Worker *w) {
    memset(w, 0, sizeof *w);
    if (avformat_open_input(&w->f, l->path, NULL, NULL) < 0) return -1;
    if (avformat_find_stream_info(w->f, NULL) < 0) return -1;
    w->si = av_find_best_stream(w->f, AVMEDIA_TYPE_VIDEO, -1, -1, NULL, 0);
    if (w->si < 0) return -1;
    w->st = w->f->streams[w->si];
    const AVCodec *codec = avcodec_find_decoder(w->st->codecpar->codec_id);
    if (!codec) return -1;
    w->d = avcodec_alloc_context3(codec);
    avcodec_parameters_to_context(w->d, w->st->codecpar);
    w->d->thread_count = l->threads;
    w->d->skip_frame = l->skip;
    if (l->lowres) w->d->lowres = l->lowres;
    if (l->lowdelay) w->d->flags |= AV_CODEC_FLAG_LOW_DELAY;
    if (!l->loopfilter) w->d->skip_loop_filter = AVDISCARD_ALL;
    if (avcodec_open2(w->d, codec, NULL) < 0) return -1;
    for (unsigned i = 0; i < w->f->nb_streams; i++)
        if ((int)i != w->si) w->f->streams[i]->discard = AVDISCARD_ALL;
    if (l->discard_demux) w->st->discard = AVDISCARD_NONKEY;
    w->pkt = av_packet_alloc();
    w->fr = av_frame_alloc();
    return 0;
}

static void worker_scale(Lab *l, Worker *w, int slot) {
    AVFrame *fr = w->fr;
    if (!w->sws) {
        w->height = ((int)(l->width * (double)fr->height / fr->width)) & ~1;
        av_image_alloc(w->dst, w->ls, l->width, w->height, AV_PIX_FMT_RGBA, 16);
        w->sws = sws_getContext(fr->width, fr->height, (enum AVPixelFormat)fr->format, l->width, w->height, AV_PIX_FMT_RGBA,
                                l->sws_flags, NULL, NULL, NULL);
    }
    sws_scale(w->sws, (const uint8_t *const *)fr->data, fr->linesize, 0, fr->height, w->dst, w->ls);
    if (l->dump) {
        char name[512];
        snprintf(name, sizeof name, "%s/%06d_%dx%d.rgba", l->dump, slot, l->width, w->height);
        FILE *o = fopen(name, "wb");
        if (o) {
            for (int y = 0; y < w->height; y++) fwrite(w->dst[0] + y * w->ls[0], 4, (size_t)l->width, o);
            fclose(o);
        }
    }
}

static void record(Lab *l, Worker *w, int slot, double started) {
    int64_t pts = w->fr->best_effort_timestamp;
    double t = pts == AV_NOPTS_VALUE ? -1 : pts * av_q2d(w->st->time_base);
    double start = w->st->start_time == AV_NOPTS_VALUE ? 0 : w->st->start_time * av_q2d(w->st->time_base);
    worker_scale(l, w, slot);
    av_frame_unref(w->fr);
    double end = now();
    pthread_mutex_lock(&l->lock);
    int rank = atomic_fetch_add(&l->arrived, 1);
    if (rank < l->count) l->arrival[rank] = end - l->t0;
    l->got_time[slot] = t < 0 ? -1 : t - start;
    double ms = (end - started) * 1000;
    if (ms > l->max_ms) l->max_ms = ms;
    pthread_mutex_unlock(&l->lock);
}

static void *seek_worker(void *arg) {
    Lab *l = arg;
    Worker w;
    if (worker_open(l, &w) < 0) return NULL;
    for (;;) {
        int rank = atomic_fetch_add(&l->next, 1);
        if (l->seconds > 0) {
            if (now() - l->t0 > l->seconds) break;
            rank %= l->count;
        }
        if (rank >= l->count) break;
        int slot = l->order[rank];
        double started = now();
        double target = slot_time(l, slot);
        int64_t base = w.st->start_time == AV_NOPTS_VALUE ? 0 : w.st->start_time;
        int64_t ts = base + av_rescale_q((int64_t)(target * AV_TIME_BASE), AV_TIME_BASE_Q, w.st->time_base);
        if (av_seek_frame(w.f, w.si, ts, AVSEEK_FLAG_BACKWARD) < 0) continue;
        avcodec_flush_buffers(w.d);
        int got = 0, reads = 0;
        while (!got && reads++ < 20000 && av_read_frame(w.f, w.pkt) >= 0) {
            if (w.pkt->stream_index == w.si) {
                if (l->drain && !(w.pkt->flags & AV_PKT_FLAG_KEY)) {
                    av_packet_unref(w.pkt);
                    continue;
                }
                avcodec_send_packet(w.d, w.pkt);
                if (l->drain) avcodec_send_packet(w.d, NULL);
                while (!got && avcodec_receive_frame(w.d, w.fr) == 0) {
                    if (l->exact && w.fr->best_effort_timestamp < ts) {
                        av_frame_unref(w.fr);
                        continue;
                    }
                    got = 1;
                }
            }
            av_packet_unref(w.pkt);
        }
        if (!got) {
            avcodec_send_packet(w.d, NULL);
            if (avcodec_receive_frame(w.d, w.fr) == 0) got = 1;
        }
        if (got) record(l, &w, slot, started);
    }
    return NULL;
}

static void sweep(Lab *l) {
    Worker w;
    if (worker_open(l, &w) < 0) return;
    int slot = 0;
    double started = now();
    double base = w.st->start_time == AV_NOPTS_VALUE ? 0 : w.st->start_time * av_q2d(w.st->time_base);
    while (slot < l->count) {
        int eof = av_read_frame(w.f, w.pkt) < 0;
        if (eof) avcodec_send_packet(w.d, NULL);
        else if (w.pkt->stream_index == w.si) avcodec_send_packet(w.d, w.pkt);
        if (!eof) av_packet_unref(w.pkt);
        while (slot < l->count && avcodec_receive_frame(w.d, w.fr) == 0) {
            double t = w.fr->best_effort_timestamp * av_q2d(w.st->time_base) - base;
            if (t >= slot_time(l, slot)) {
                while (slot + 1 < l->count && t >= slot_time(l, slot + 1)) slot++;
                record(l, &w, slot, started);
                started = now();
                slot++;
            } else {
                av_frame_unref(w.fr);
            }
        }
        if (eof) break;
    }
}

static long proc_read_bytes(void) {
    FILE *f = fopen("/proc/self/io", "r");
    char line[128];
    long v = -1;
    while (f && fgets(line, sizeof line, f))
        if (sscanf(line, "read_bytes: %ld", &v) == 1) break;
    if (f) fclose(f);
    return v;
}

static long proc_rchar(void) {
    FILE *f = fopen("/proc/self/io", "r");
    char line[128];
    long v = -1;
    while (f && fgets(line, sizeof line, f))
        if (sscanf(line, "rchar: %ld", &v) == 1) break;
    if (f) fclose(f);
    return v;
}

int main(int argc, char **argv) {
    if (argc < 2) return 2;
    Lab l;
    memset(&l, 0, sizeof l);
    l.path = argv[1];
    l.count = 100;
    l.width = 320;
    l.workers = 1;
    l.threads = 1;
    l.skip = AVDISCARD_NONKEY;
    l.sws_flags = SWS_FAST_BILINEAR;
    const char *mode = "key", *order = "levels", *skipname = NULL, *scaler = "fast";
    for (int i = 2; i + 1 < argc; i += 2) {
        const char *k = argv[i], *v = argv[i + 1];
        if (!strcmp(k, "--count")) l.count = atoi(v);
        else if (!strcmp(k, "--width")) l.width = atoi(v);
        else if (!strcmp(k, "--mode")) mode = v;
        else if (!strcmp(k, "--order")) order = v;
        else if (!strcmp(k, "--workers")) l.workers = atoi(v);
        else if (!strcmp(k, "--threads")) l.threads = atoi(v);
        else if (!strcmp(k, "--skip")) skipname = v;
        else if (!strcmp(k, "--loopfilter")) l.loopfilter = atoi(v);
        else if (!strcmp(k, "--discard")) l.discard_demux = atoi(v);
        else if (!strcmp(k, "--lowres")) l.lowres = atoi(v);
        else if (!strcmp(k, "--drain")) l.drain = atoi(v);
        else if (!strcmp(k, "--lowdelay")) l.lowdelay = atoi(v);
        else if (!strcmp(k, "--seconds")) l.seconds = atof(v);
        else if (!strcmp(k, "--dump")) l.dump = v;
        else if (!strcmp(k, "--scaler")) scaler = v;
    }
    l.exact = !strcmp(mode, "exact");
    l.sweep = !strcmp(mode, "sweep");
    if (l.exact || l.sweep) l.skip = AVDISCARD_DEFAULT;
    if (skipname) {
        l.skip = !strcmp(skipname, "nokey")    ? AVDISCARD_NONKEY
                 : !strcmp(skipname, "nonref") ? AVDISCARD_NONREF
                 : !strcmp(skipname, "bidir")  ? AVDISCARD_BIDIR
                                               : AVDISCARD_DEFAULT;
    }
    if (!strcmp(scaler, "bilinear")) l.sws_flags = SWS_BILINEAR;
    if (!strcmp(scaler, "bicubic")) l.sws_flags = SWS_BICUBIC;
    av_log_set_level(AV_LOG_QUIET);

    l.t0 = now();
    long read0 = proc_read_bytes(), rchar0 = proc_rchar();
    AVFormatContext *probe = NULL;
    if (avformat_open_input(&probe, l.path, NULL, NULL) < 0) return 3;
    if (avformat_find_stream_info(probe, NULL) < 0) return 3;
    int si = av_find_best_stream(probe, AVMEDIA_TYPE_VIDEO, -1, -1, NULL, 0);
    if (si < 0) return 3;
    int entries = avformat_index_get_entries_count(probe->streams[si]), keys = 0;
    for (int i = 0; i < entries; i++) {
        const AVIndexEntry *e = avformat_index_get_entry(probe->streams[si], i);
        if (e && (e->flags & AVINDEX_KEYFRAME)) keys++;
    }
    const char *codec = avcodec_get_name(probe->streams[si]->codecpar->codec_id);
    l.duration = probe->duration / (double)AV_TIME_BASE;
    double opened = now() - l.t0;
    avformat_close_input(&probe);

    l.order = calloc((size_t)l.count + 8, sizeof(int));
    l.arrival = calloc((size_t)l.count + 8, sizeof(double));
    l.got_time = calloc((size_t)l.count + 8, sizeof(double));
    for (int i = 0; i < l.count; i++) l.got_time[i] = -2;
    pthread_mutex_init(&l.lock, NULL);
    if (!strcmp(order, "rev")) order_rev(l.order, l.count);
    else if (!strcmp(order, "levels")) order_levels(l.order, l.count);
    else
        for (int i = 0; i < l.count; i++) l.order[i] = i;

    if (l.sweep) {
        sweep(&l);
    } else {
        pthread_t th[32];
        int n = l.workers > 32 ? 32 : l.workers;
        for (int i = 0; i < n; i++) pthread_create(&th[i], NULL, seek_worker, &l);
        for (int i = 0; i < n; i++) pthread_join(th[i], NULL);
    }
    double total = now() - l.t0;

    int done = atomic_load(&l.arrived);
    int all_done = done;
    if (done > l.count) done = l.count;
    double err = 0, worst = 0;
    int measured = 0;
    double prev = -1;
    int unique = 0;
    double *times = calloc((size_t)l.count + 1, sizeof(double));
    int tn = 0;
    for (int s = 0; s < l.count; s++) {
        double g = l.got_time[s];
        if (g < 0) continue;
        double e = g > slot_time(&l, s) ? g - slot_time(&l, s) : slot_time(&l, s) - g;
        err += e;
        if (e > worst) worst = e;
        measured++;
        times[tn++] = g;
    }
    for (int i = 0; i < tn; i++)
        for (int j = i + 1; j < tn; j++)
            if (times[j] < times[i]) {
                double x = times[i];
                times[i] = times[j];
                times[j] = x;
            }
    for (int i = 0; i < tn; i++) {
        if (times[i] != prev) unique++;
        prev = times[i];
    }
    struct rusage ru;
    getrusage(RUSAGE_SELF, &ru);
    double cpu = ru.ru_utime.tv_sec + ru.ru_utime.tv_usec / 1e6 + ru.ru_stime.tv_sec + ru.ru_stime.tv_usec / 1e6;
    int p25 = done / 4, p50 = done / 2;
    printf("{\"thumbs\":%d,\"codec\":\"%s\",\"duration\":%.2f,\"index_entries\":%d,\"index_keys\":%d,\"open_s\":%.4f,\"wanted\":%d,\"done\":%d,"
           "\"unique\":%d,\"t_first\":%.4f,\"t_5\":%.4f,\"t_25pct\":%.4f,\"t_50pct\":%.4f,\"t_all\":%.4f,\"per_ms\":%.2f,"
           "\"max_ms\":%.2f,\"err_mean_s\":%.3f,\"err_max_s\":%.3f,\"cpu_s\":%.3f,\"read_mb\":%.1f,\"rchar_mb\":%.1f,\"rss_mb\":%.0f}\n",
           all_done, codec, l.duration, entries, keys, opened, l.count, done, unique, done ? l.arrival[0] : -1,
           done >= 5 ? l.arrival[4] : -1, done ? l.arrival[p25] : -1, done ? l.arrival[p50] : -1, total,
           done ? 1000 * (total - opened) / done : -1, l.max_ms, measured ? err / measured : -1, worst, cpu,
           (proc_read_bytes() - read0) / 1048576.0, (proc_rchar() - rchar0) / 1048576.0, ru.ru_maxrss / 1024.0);
    return 0;
}
