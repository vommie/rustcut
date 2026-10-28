# Changelog

All notable changes to RustCut are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A loaded job whose folder is not among the target folders keeps it as the temporary entry "Job target" (first in
  the folder list): saving without choosing another folder writes there again. It gives way to your previous choice
  when you open another video, and a banner (instead of a toast) offers "Add to target folders" meanwhile (0.0.81).

- Settings > Target Folders: the target folders as a tree. Groups (menu entries without a path) hold folders and other
  groups at any depth; drag an entry before, after or into a group; F2 or a double click renames it; new folders come
  from the folder dialog or are dropped from the file manager. Deleting is held (2 s); a group goes with everything in
  it, and the button's tooltip says so (0.0.78).
- Every entry can have a picture (the first letter, one of about 240 built-in icons, or an own image or SVG) and a
  color. Own images are stored small (256 px, WebP); own SVGs are cleaned of scripts and external references first.
  They live in `pictures.sqlite` in RustCut's data folder; pictures no entry uses any more are removed (0.0.78).
- A star makes an entry a favorite with its own place on the folder wheel's first level; the details show the wheel's
  places: a click or dragging an entry from the tree onto a place puts it there, the small "×" on a place frees it. In a
  narrow Settings window the details stand below the tree, and the page scrolls while an entry is dragged near its top
  or bottom edge (0.0.78).
- The Job Finalization card's target folder field shows the folder's picture and opens a searchable list: "Next to the
  source", your favorites, the last three folders jobs were saved into, then the whole tree (folded groups stay
  folded). Typing searches every level and shows each hit's group; a folder that does not exist yet has a warning
  mark. Up/Down, Left/Right (fold), Enter and Escape work in the list (0.0.79).
- The folder wheel: hold the middle mouse button over the video (a ring fills for 250 ms) or press F anywhere, and a
  wheel of your target folders opens in the middle of the video, sized to the window (40 % of its shorter side).
  Point at a folder and let go to choose it; rest on a group (its field fills) or push past its edge to open it; the
  middle goes one level up after the same rest, and letting go there cancels. Let go over a group and the wheel stays
  for clicks. Opened with F, A/D or Left/Right turn it, W/Up or Enter opens a group, S/Down goes back, Enter chooses,
  F again goes up (and closes at the start), Escape closes. Its first level shows your favorites on their places
  (without favorites it is empty and says how to add them, 0.0.81; "Next to the source" is on it only with a star);
  crowded levels end with "More ...". Choosing never saves the job. Settings > Target Folders has its options, the
  key is in Settings > Hotkeys ("Open the folder wheel") (0.0.80).
- How the folder pictures and colors show in the wheel and the lists (shapes, color mode) (0.0.78).

### Changed

- The target folders are now a folder tree (the first step of the new target folder picker): your folders move into
  it in their order, without groups or favorites. `config.toml` is migrated to version 8 (`folder-tree`) and
  `ui-state.toml` to version 2 (`target-folder-id`, the chosen folder is kept by an id); both files are backed up
  first (0.0.77).
- "Source folder" in the Job Finalization card is now called "Next to the source" (0.0.77).
- The target folders moved from Settings > Export & Naming to their own page, Settings > Target Folders; the Job
  Finalization card's edit button leads there (0.0.78).
- A loaded job whose folder is not among the target folders keeps that folder in the card and a message offers "Add to
  target folders"; the folder is no longer added silently (0.0.77).

### Fixed

- Settings > Export & Naming no longer checks every target folder on the window's thread while it draws the list;
  the check runs in the background (0.0.77).

## [0.0.76] - 2026-10-28

The tag history follows your saved jobs: it changes only on a save and shows how long ago each tag was used; disabled
tags look disabled.

### Changed

- The tag history changes only when a job is saved: clicking a tag (in the tree or the history) no longer adds it or
  moves it, since it may be removed again before saving. A save moves the job's tags to the front together, the last
  clicked first (0.0.76).
- The tag history shows how long ago each tag was saved: the last saved job's tags are fully visible, tags last saved
  two, three and four jobs ago fade to 85, 75 and 65 %, older ones stay at 55 %. A job saved without tags changes
  nothing. `ui-state.toml` keeps the count beside the history (`history_age`); a file of an older version counts the
  last job's tags as new and the others as one job older (0.0.76).

### Fixed

- Tags in the tag history look disabled (gray text, no highlight under the pointer) while they cannot be assigned,
  for example without a video; before, only the faint tags of older jobs looked different. Older disabled tags are
  gray and faint (0.0.76).

## [0.0.75] - 2026-10-28

Your UI review "RustCut Feedback" and the requests that came with it: the transport bar, the symbols over the video,
the rating keys, the quick crop on F4, framed dialogs, the tag history's glide and many small fixes.

### Added

- The last saved job's rating button lights up under the pointer, darkens while pressed and is filled while the
  current job has that rating (0.0.75).
- A refresh button over the tag tree's "RustView's library has no tags yet" hint (the tree's Refresh Tag Tree, which
  needs a tree for its context menu) (0.0.74).

- A tag used again glides to the front of the tag history, also from the last row, and the other tags slide to their
  new places, so the eye can follow what changed (0.0.73).

- Buttons for the sections under the timeline, after the frame steps: set the start, set the end, add the section. They
  are off without a video, and Add is off until start and end make a section of at least one frame.
- The volume bar over the lower part of the video when the mouse wheel changes the volume, and a speaker in the
  middle of the video when the mute key is pressed, like RustView (the slider and the mute button show it themselves).
  The symbols in the middle of the video (play, pause, mute, rating) share one look and one timing. Each symbol has
  its own switch in Settings > Player > Symbols over the video (new settings `player.flash_mute`,
  `player.flash_rating`, `player.volume_bar`, on by default; older `config.toml` files load unchanged).
- A collapsed sidebar shows a pin on its expand button while a panel is held over the video. Each side holds its own
  panel, and the hold is kept while the sidebar is expanded and across restarts (`ui-state.toml`, older files load
  unchanged).
- A toggle under the tag history (left of the eye) hides the "Filter tags..." field and gives its room to the tag
  tree; while it is hidden the filter does not apply. The choice is kept (`ui-state.toml`, older files show the field).
- The tag history shows the tags the last saved job did not have a little faint, so you see at once what "Apply the
  last saved job's tags" brings back.
- The file dialogs show how long ago a file was modified: "(today)", "(yesterday)", "(5 weeks ago)", "(3 years ago)".
- The auto-crop button wears a round badge of the border color it looks for, unless that color is pure black.
- The volume slider clings to 25, 50, 75 and 100 % (Ctrl drags freely).
- The crop's four edges and the resize's width and height step with the mouse wheel at once while their filter is on
  (the field opens and takes the focus; other fields still step only once they are open), and a click into one of
  them while the filter is off switches the filter on.
- The queue follows new jobs while it stands at the bottom (0.0.74: it stopped following after the first new job); scrolled up it stays put until it is scrolled to the bottom
  again. It starts at the bottom. Settings > Queue > "Follow new jobs" switches it off (new setting
  `queue.follow_new_jobs`, on by default; older `config.toml` files load unchanged).
- Ctrl+0 to Ctrl+5 set the rating, like in RustView (Settings > Hotkeys > Tag & Rate: "Rate 0" to "Rate 5"); the
  rating's digit shows briefly in the middle of the video (not when a rating button is clicked). "Fit timeline" moves
  from Ctrl+0 to Ctrl+Backspace and Backspace (RustView's zoom reset keys); a key you set yourself stays.

### Changed

- The quick crop has its own key, F4 (Settings > Hotkeys > Filters > "Quick crop"): F4 starts it, drag the nearest
  edge or corner, F4 again or Enter keeps it, Escape restores. Holding Ctrl alone does nothing any more (it is only a
  modifier, so Ctrl+0..5 and Ctrl+O never start a crop).

- The volume goes from 0 to 100 % (no amplification up to 150 % any more); a stored volume above 100 % becomes 100 %.
- The transport bar: the times stand on the far right, the volume left of them.
- In a narrow queue (below 450 px) a job's state is its colored symbol; from 450 px the symbol and the word. The row's
  tooltip names the state.
- The conflict dialog opens just tall enough for its Decision tab (512 px instead of 560 px; a size you set stays).
- The conflict dialog's choices: Next free number, Add "(n)", Own name, Overwrite (in this order), with a subtle line
  between them. The key of "Next free number" is green while the counter auto-iterates, the key of "Overwrite" red.
- While the counter auto-iterates, the files of the name's series with a lower number (the jobs saved before) and the
  name without a number are no "similar filename" hint any more; equal and higher numbers still are (0.0.75: the
  name without a number kept the hint).
- Rating buttons are 20 px high, dark with their color as the border, filled with the color on hover and when chosen;
  the chosen one has no white border any more and gets a little lighter under the pointer (0.0.74); also in the
  question before saving without a rating. RustView gets the same look at the release.
- File rows in the dialogs light up and open only on their picture and their buttons, not on the whole row.
- Every dialog shows its content in one framed panel between its head and its buttons; in the dialogs with tabs the
  tabs sit on that frame and the chosen tab opens into it, so the tab bar no longer floats.
- The Probe Render card's "Exact settings" button shows a ruler (the target with the arrow is for jump marks only).
- The question before saving a job without a rating starts with the focus on the rating 5 (Enter saves with 5).
- "Hold for" takes 100 ms and more (it was 300 ms; 0 still means a click is enough).
- A filter row shows the move cursor only while it is dragged, not when the pointer rests on it or the wheel turns.
- The crop is dragged in steps of one pixel; "Divisible by" applies only to the finished video's size.
- The playhead's line (below its head) is exactly one pixel wide (0.0.75: it was drawn about three pixels wide), and so
  are the crop's frame lines, also while an edge is dragged.
- A link in a message bubble lights up and acts only under the pointer, not when the pointer is anywhere on the bubble.
- The Sections card has no hint text when it has no sections; the delete button of a section has room after the
  duration.

### Fixed

- The faint pause symbol no longer shows after a frame step or when the video stands at its end; back from the end and
  not playing, it is paused again. While a key's symbol (mute, rating) shows it steps aside and comes back afterwards.
- A tooltip no longer flies in from the place of the previous one.
- The queue's filter menu keeps the typed search when it is opened again.
- One can type into "Own name" in the conflict dialog again (a click put the focus there and took it away at once).

## [0.0.71] - 2026-10-27

The tag tree's Compact Mode, RustView's context menu on the tree's free space and a real Refresh Tag Tree.

### Added

- Compact Mode for the tag tree: dense rows that share one line, in PyCutRv's font size, with the same marks, lock look,
  expand buttons and indentation, a slightly darker row background and a lighter hover; text, lines and symbols sit on
  whole pixels and stay sharp. Right-click the tree's free space and choose "Compact Mode" ("Normal Mode" switches
  back), or use Settings > RustView > Tags and rating > Compact tag tree. The choice is kept (new setting
  `rustview.compact_tree`, off by default; older `config.toml` files load unchanged, no migration). Normal Mode looks
  exactly as before (0.0.70, tuned after your test in 0.0.71).
- The tag tree's free space has RustView's context menu: Expand All, Collapse All, Refresh Tag Tree and the mode switch
  (RustCut never adds tags, so there is no "Add Root Tag"). Refresh Tag Tree asks RustView for its tags now and changes
  the tree only when they differ, in place: the chosen, locked and expanded tags that still exist stay (0.0.70).

## [0.0.69] - 2026-10-27

The playhead's head and line move together to the pixel.

### Fixed

- The playhead's head wobbled against its line while it glided: head and line are one shape now and move together to
  the pixel (0.0.69).

## [0.0.68] - 2026-10-27

The user's review of the Settings window (0.0.67) and the editor issues of 2026-10-27 (0.0.68): the counter, RustView's
section marks, exact timeline seeks, a gliding playhead, the tag tree's expand buttons and known sources like PyCutRv.

### Added

- The start mark is at 0 in every newly opened video (as in RustView); a loaded job with sections makes its first
  section the current one (its marks) and the player goes to its start.

- Settings > Render Profiles: "+" makes a new profile with the standard settings (no copy of the selected one), and
  the eye hides a profile from the Codecs panel's choice and "Render again with" (it stays in Settings; the profile new
  jobs start with is always offered). The buttons under the list: duplicate, delete, star, eye on the left, "+" on
  the right. New setting `render.hidden_profiles` (older files load with nothing hidden).
- Settings > Advanced & Logs > Logs: "Use at most" (default 100 MB) limits the space of the log files; the oldest go
  first. The files of earlier days are packed as `.log.gz`. New setting `advanced.log_max_mb`.
- Settings > RustView > "Test the connection" answers every time: a turning symbol while RustView is asked (the
  button waits), then a green check or a red cross with the reason beside the button and under the row. Before a
  test the row shows the link's state.
- The program rows (RustView's program, ffmpeg, ffprobe) say whether the program works and where it was found
  ("Found in PATH: /usr/bin/ffmpeg (ffmpeg version 9.0.1)"), or what is missing; the texts say that the fields are
  optional.
- A message when RustView refuses a job's tags and rating ("Job's tags not written to RustView", a toast with the
  reason and a link to the job; Settings > Messages).
- Settings > Hotkeys: a held "Reset all keys" button beside the filter.
- Settings > Player > Hardware decoding: a list of mpv's values beside the field (typing any value still works); the
  log level has one too.

### Changed

- Job Finalization: an empty counter field counts as 01 while "The counter goes up after each save" is on (the saved
  file gets " - 01", the field then shows 02); only with auto-increment off does an empty field mean no counter.
- Section marks follow RustView's rules: an end without a start puts the start at 0, an end before the start moves the
  start onto it, and start and end may share a time (a mark on the other is no longer dropped).
- Opening a video RustView does not know registers its MD5 in RustView at once, as PyCutRv does (while Tag & Rate is
  "Active" and the known-source message is shown): opening it again, in RustCut or PyCutRv, warns that it is known.
  A video is known as soon as RustView knows its hash, also when the video itself is in the library; the banner says
  "RustView knows this video" and when it became known (Settings > Messages: "Source known to RustView").
- The playhead glides while the video plays: it moves on between mpv's position reports (every 8 ms, at most 200 ms
  ahead), so it no longer jumps a few pixels per frame on short videos.
- Play/pause uses mpv's own toggle with the observed pause and end state (the command thread no longer reads
  properties).

- "Hold for" (Settings > General) is 1000 ms by default (was 2000 ms). A value saved in `config.toml` stays.
- The queue panel's "When the queue finishes" button and Settings > Queue > "Then" are one choice: a change in either
  place shows in the other and is kept for the next start (before, the panel's choice was kept apart in
  `ui-state.toml` and hid the setting after the first quit). After quitting, suspending or powering off it goes back
  to "Do Nothing". Both use the same words.
- The page "Warnings & Confirmations" is gone: its four options are kinds on Settings > Messages > Message kinds
  ("Question: the target file exists", "Question: a queued job writes the same file", "Question: no tags or rating",
  and "Similar names in the target folder"), with "Dialog" (or "Bubble") or "Nothing". The values stay in
  `[warnings]`, so nothing is migrated. The link card "When a video opens" and the "Notifications" card of Desktop
  Integration (they only led to Messages) are removed.
- Every Settings page keeps its own scroll position while RustCut runs; a page opened for the first time starts at
  its top. The pages scroll with a drag or swipe, too, and have a scrollbar of normal width; the page list scrolls
  when the window is too low.
- The quality slider's words ("High - CRF 23") stand above the slider, so the pointer never covers them (Settings and
  the Codecs panel).
- The Open buttons of Advanced & Logs show the "open in another program" symbol; the Close button has a normal size;
  the Diagnostics copy button says "Copy the report".
- The Settings window comes back where it was (its position is kept in `ui-state.toml`, like its size).

### Fixed

- A click on the timeline made the video jump twice (to a keyframe, then to the exact frame), and a drag ended with an
  extra jump: every timeline seek goes to the exact frame now, and the release sends no second seek to the same time.
- A parent tag in the Tag & Rate tree sometimes showed "+" while it was expanded (or "-" while collapsed): a tree row
  is made again when its tag or expansion changes, so the button follows the tree again.
- At the end of a video the playhead stepped back to the last frame's start and never reached the end; it stands at
  the end now.

- The last card of a Settings page (e.g. RustView's "Source hashes") could not be scrolled into view; its text had a
  gap of blanks in the middle.
- The log's daily cleanup counted the shareable copy's files with the log's, so it could delete them early.

## [0.0.66] - 2026-10-27

The user's UI review round 2: filters, option menus and the queue; a faster queue list; the maximized window comes
back; the taskbar shows the current job.

### Added

- Every job state has its own colored symbol (0.0.62): waiting (hourglass), running (play), paused, held (hand),
  finished (check), failed (alert) and canceled (ban). The queue's filter shows them before the states.
- A finished job's line in the compact queue shows the size of its output (0.0.62).

### Changed

- The resize's width and height fields step with the mouse wheel once clicked into, like the crop's edges: 1 pixel,
  10 with Ctrl, 100 with Shift (also the Up/Down keys; before, a step was 16 pixels) (0.0.66).
- The taskbar entry's progress bar follows the current job by default (0.0.65): Settings > Desktop Integration >
  "The bar shows" chooses between the current job (the render that started first when several run) and all jobs
  (the whole run, as before). Older settings files load with "Current job".

- The queue handles hundreds of jobs smoothly (0.0.64): it creates only the rows in view and updates only the rows
  that changed (before, every row was made and rewritten up to ten times a second while rendering).
- The symbols of the queue's totals line say "3/4 finished" in their tooltips: of every job in the queue (0.0.64).
- The queue's totals line shows a colored symbol per state with its count in a small badge (only states that occur)
  instead of words, and "~0:22 left" instead of "about 0:22 left" (0.0.62).
- File sizes in the queue and the Job panel are in binary units with at most one decimal ("324 KiB", "2.1 MiB",
  "2.3 GiB") (0.0.62).
- "Pause after the running jobs" is off while nothing renders or processing is paused (0.0.62).
- The filters' options menus line up on two equal columns (0.0.61): every choice, number field and slider ends on one
  line; Deshake's values sit in a grid of two halves, its segmented choices span the whole row, and "Field order"
  (Deinterlace) and "Search" (Crop) are drop-down lists.
- Segmented choices show a faint line between two parts that are not chosen (0.0.61).
- The Deshake row of the Filters card shows shakiness and smoothing with small symbols instead of words; that Deshake
  renders only (not in the preview) is now in the tooltip of its name instead of an extra icon (0.0.61).
- The crop's four edge fields step with the mouse wheel once clicked into: 1 pixel, 10 with Ctrl, 100 with Shift
  (also the Up/Down keys) (0.0.61).
- Sections: the button that sets the end now sits left of the end, like the start's (0.0.61).
- The timeline's marks are plain thick lines, without the flags (0.0.61).
- The output name field is as high as the counter and the button beside it (0.0.61).
- Settings > Filters > Crop ratios: the built-in list no longer has 3:2, 16:10, 21:9 and 2:3 (0.0.61). Migration
  `crop-ratios-trimmed` (settings version 7, `config.toml` is backed up first): a list that is still the old built-in
  one becomes the new one; a list you changed stays as it is.

### Fixed

- A window that was maximized when RustCut ended opens maximized again (0.0.63); before, it opened at its windowed
  size although the state was saved. Un-maximizing returns to the saved size.

## [0.0.60] - 2026-10-27

The first release of RustCut.

### Added

- Choices with at most three short entries show them side by side, the chosen one pressed in (0.0.36): for example
  the bit depth in the Codecs panel and "Automatic zoom" in Settings > Filters.
- Up and down are one button wherever things move (0.0.36): its upper half moves up, its lower half down (the queue's
  jobs, the sections, the target folders in Settings > Export & Naming).

- "The file exists" and "Job #n writes this file too" under the name field offer a button that solves it (0.0.35): a
  name with a numbering scheme jumps to the next free number (the counter is switched on when it was off), any other
  name gets " (n)" with the first free number, like a file manager. The button's tooltip says what it does.
- A paused queue says so at the start (0.0.35): when jobs wait, a banner says why (it was paused when RustCut ended,
  or "Start paused") and has "Resume". Settings > Messages > "Queue paused at the start" makes it a toast, a desktop
  notification or nothing.
- A tag filter window (0.0.34): the filter button under the tag history opens RustView's whole tag tree with a check
  box per tag. Unchecked tags are hidden in the Tag & Rate panel with everything below them, also tags RustView adds
  there later; a tag below a hidden one can be checked again on its own (the hidden tags above it stay in the tree,
  dimmed, as the way to it). A search shows the matching tags with the way to them, branches fold, the number on the
  filter button says how many tags are hidden, and "Show all" (held) shows every tag again. Every change shows in the
  panel at once; the window remembers its size and folded branches. The eye button still hides and shows branches
  right in the tree, now with the same rules.

- The Tag & Rate panel has PyCutRv's switches in a row under the tag history (0.0.33): **Active** (the tag symbol)
  and **Warn** (the warning triangle). Switched off, Active saves the next jobs without RustView: RustView gets no
  entry, no tags, no rating and not the video's MD5, also when the job finishes; the panel is dimmed meanwhile and the
  Job panel says "Saved without RustView". Warn asks before a job without tags or rating is saved. Both are switched
  off by holding them and stay red while off (a click switches them on again); Active stays off after a restart. When
  one of them is still off as RustView answers for the first time after a start, a banner says so once, with "Switch
  on" for both (Settings > Messages > "Tagging switched off at the start" can make it a toast).

- A color picker for the colors of the Settings window (0.0.31, taken over from RustView): the Background row shows
  the color as a swatch; a click opens the picker in its own window over the Settings window (it stays above it),
  with a saturation/value area, hue slider, hex, RGB and HSV fields, a palette, the original color and "Default".
  Every change shows at once; Cancel, Escape or closing the window restore the color, OK keeps it.
- "Without sections" (Settings > Editor & Sections) has PyCutRv's choice "Marked range, else whole video" (0.0.31):
  Save without a listed section takes the range between the marks, or the whole video when no mark is set.

- A failed job's toast leads to the job (0.0.29): a click shows the queue panel, selects the job, scrolls it into view
  and lets it flash (a filter that hides it is cleared). Saving without a section shows "The job was not saved" with
  a button "Show the sections" that scrolls to the Sections card.

- Dialogs for a start that cannot go on as usual (0.0.28). **Settings from a newer RustCut**: when `config.toml` or
  `ui-state.toml` was written by a newer RustCut, a dialog (one tab per file) says so, lists the settings this version
  does not know and offers: continue without saving (the default; the Settings window says so at its top), back up the
  file and save it with this version, back up the file and start with defaults, or quit; both tabs' choices apply when
  you confirm, and "Open folder" shows the files. **The queue cannot be opened**: when the queue database is damaged,
  from a newer RustCut or out of reach, a dialog offers to rescue the readable jobs into a new queue, back the file up
  and start empty, continue with a temporary queue, try again or quit (the fitting way is preselected), plus "Open
  folder" and "Copy details". The old file is always kept in the backup folder. While the queue is temporary, a red
  line at the top of the queue panel says so (a click shows the ways again), and quitting with unfinished jobs asks
  first ("Quit and lose them" has to be held).
- "This video was cut before" has a button "Show N files" (0.0.28): a dialog lists every file RustView knows was made
  from the open video, with its picture, folder, length, size, date, rating and tags, "file missing" when it is gone
  and the queued job that writes it, each with play and "show in the file manager". A second tab shows the files in
  the target folder whose names look like the video's (the same list as the conflict dialog's). A playing video pauses
  while the dialog is open and plays on when it closes. A source RustView knows without files gets the banner alone;
  a job loaded into the editor never shows it.

- Bubbles at the field a message is about (0.0.27): a small closable note like a tooltip, with a colored stripe for
  its level, that points at its field and stays until the problem is solved; × hides it until its words change. They
  show for the job's target name in Job Finalization ("The file exists", "Job #3 writes this file too", similar names
  in the folder; a click on it opens the target folder's files with their pictures, to look, with the queued jobs that
  write the file), a target folder that cannot be read, a typed section time that is no time, sections that overlap
  (at the later one), every error and warning of the render settings at its option in the Codecs panel (at the profile
  choice when the option's card is folded) and a typed value the Codecs panel or the Settings window refuses. While you
  type, a check waits until you pause (0.6 s), and a bubble that shows changes its words in place. A bubble stays
  inside its panel, moves with scrolling, goes when its panel or card is hidden and turns above its field when there is
  no room below. The mouse wheel over a bubble makes it more transparent (down) or more opaque (up), to see what lies
  under it; Settings > Messages > Look & behavior has its start opacity (`[notices.bubbles] opacity`, 20-100 %,
  default 100). The bubble kinds are listed on the Message kinds tab; their test shows the bubble at the test button.
- The interlace note of the Deinterlace filter explains itself in its tooltip: the verdict and how many frames ffmpeg
  counted top field first, bottom field first, progressive and undetermined. A new message kind "Interlaced video
  found" can show it as a toast or banner with a button that switches Deinterlace on (Settings > Messages; by default
  only the note says it).

- Settings > Messages (0.0.26), a page with three tabs. "Look & behavior": where toasts appear, how several stack,
  how many show, their order, animation and time per level; how banners come and whether a new error lights up; the
  bell and its number; desktop notifications and their time. "Message kinds": every kind of message RustCut shows,
  grouped, with an example, where it appears (only the places that suit it; critical messages are locked), whether it
  also goes to the desktop, a test button that shows the real message at its real place (the row says where it went)
  and "Don't show again" for hints and confirmations. "Muted": what you muted, with "Show again". A search filters the
  kinds and the muted list; the tab and the search are remembered. The page's held reset button also resets every
  kind's place and the muted list.

- A notice system for every message (0.0.25). **Toasts**: short cards at the top right of the window that leave by
  themselves (hints and confirmations after 5 s, warnings after 8 s, errors stay until closed). Each shows its symbol,
  a title, a detail line, its buttons, the time, a line with the time left and "×3" when the same message came again.
  The pointer on a card stops its time and shows the whole text; a click leads to the cause, × closes it, a right click
  offers "Copy the text", "Don't show this kind again" (hints and confirmations only, also as a small bell button on
  the card) and "Close all messages". At most three show at once, the most important first, "+2 more" opens the rest.
  **Banners**: states that hold lie over the top of the video, full width, tinted in their level's color, with their
  buttons: "The job cannot be saved" (with a button to the option), "The job was not saved", a file that does not open
  or play (with "Show log" and "Reload"), "This video was cut before", the probe render that plays (with "Back to the
  source"), ffmpeg or ffprobe that cannot be started (new; with a button to the programs' settings). Several banners
  show one at a time with "‹ 1 / 3 ›"; × hides a banner until its state changes; a new error lights up once.
  **The bell** at the right end of the menu bar counts the unread errors and warnings and lists the last three
  messages; "Show all" opens the Logs panel's new "Messages" log of this session.
- The probe render shows its progress in a toast with a Stop button (and in the Probe card as before), then "Probe
  render ready" with the size.
- `config.toml` has a `[notices]` section for all of it: the toasts' place (four on the video, two in the window), how
  several stack (some with "+N more", all, one at a time with a pager, a deck that fans out under the pointer, a
  summary card), how many show (2-5), their order, their animation and their time per level; the banners' animation
  (curtain, fade, none) and whether a new error lights up; the bell and what it counts; and for each kind of message
  where it appears and whether it also goes to the desktop. Errors also go to the desktop while RustCut is not in
  front. The Settings window's page for all of this follows.

- Jump marks (0.0.23): a message about an option has a small target button that leads straight to it. The panel or
  window opens, a folded card unfolds, the option scrolls into view and flashes three times. The Tag & Rate panel's
  header uses it while RustView cannot be reached (Settings > RustView, the program) or is switched off (the
  switch).

- New tooltips everywhere (0.0.21): one modern bubble with a small arrow that points at its button. The keys of a
  button sit on their own line under the text as key caps ("Key [Ctrl] [S]"; an action with two keys shows "Keys [,]
  or [PgUp]"; the arrow keys as arrows). Long explanations get a title and rows, e.g. the deinterlacer, the scaling
  algorithm and the deshake zoom list every choice with what it does. The bubble finds its place by itself: centered
  under the button, moved inward at the window's edges, above the button at the bottom of the window, beside the
  button when neither fits; a context menu's tooltip appears beside the menu so it never covers other entries. The
  first tooltip waits for the delay from Settings > Appearance, the next one along a row of buttons shows at once,
  and it fades in softly.
- The player's buttons (play/pause, frame steps, mute) have tooltips with their keys.

- The "file exists" dialog has tabs (0.0.20). "Decision" shows the existing file with a picture (a click plays it,
  the "i" in its corner shows date, size, length, resolution, codecs and bit rate) and every way out: next free number,
  overwrite, "(n)" and your own name, or the ways for queued jobs. "Similar files" lists every file of the name's
  numbered series ("Name - 01.mkv", "Name - 02.mp4", ...) in any video format and the other videos whose names start
  with the name, each with a picture, play and show-in-folder buttons, in two groups you can fold. "Queue" lists the
  jobs that write the same file when it also exists. Tab and Shift+Tab switch tabs.
- Pictures of the files are made in the background (the existing file first, then the list), a placeholder shows
  until each arrives, a file without one says why (audio only, not a video, not readable); "Reading 12 of 40 files"
  shows how far it is. Closing the dialog stops the work at once.
- Every dialog can be resized at its edges and corners; the size is remembered (`ui-state.toml`, `[dialogs]`), and
  switching tabs never changes it. The folded groups are remembered too.
- Settings > Dialogs: the way out chosen when the dialog opens, whether the series and the similar names are listed,
  which files get pictures (every file, the existing file only, none), where in a video the picture is taken, how many
  pictures are made at once and their size in the list (`[dialogs.conflict]` in `config.toml`).
- MD5 sidecars: when RustCut had to read a video for its MD5, it writes `.<name>.md5` next to it (md5sum's format), so
  neither RustCut nor other programs read it again; an existing sidecar is never changed. Settings > RustView > "Write a
  sidecar" (`[hashing] write_sidecar`, on).

### Added

- A final size rule (0.0.60, your questionnaire about the final size): Settings > Filters > Final size > "Divisible
  by" (Off, 2, 4, 8, 16; 2 by default) acts on the finished video, after every filter in the order of the Filters
  card, and cuts the rest evenly from both sides. "Off" keeps odd sizes (H.264 and H.265 refuse them, AV1 and VP9
  take them). The Filters card's header shows the final size; its tooltip says it is the final size, and only when
  the rule cut something it shows the size before and after and a jump symbol leads to the setting. Migration:
  `config.toml` goes to version 6 ("final-size-rule", backed up first): your Crop "Divisible by" value becomes the
  final size rule. Jobs saved before keep the even rule they rendered with.
- Deshake sharpens lightly after stabilizing (0.0.59, your night questionnaire): every moved frame is computed anew and
  comes out a little softer, so the render adds the sharpening the ffmpeg manual recommends right after the
  stabilization. "Sharpen" in the Deshake menu and in Settings > Filters switches it, on by default. Migration:
  `config.toml` goes to version 5 ("deshake-sharpen-on", backed up first) and switches it on in your saved Deshake
  defaults; jobs saved before still render without it, as they did.
- A shareable log (0.0.55, your UI review's note on the whole review): beside the log, RustCut writes
  `rustcut-shareable.DATE.log`, the same lines with file names, folders and paths, tag names, user and host names, MD5
  sums and the videos' titles replaced by placeholders that stay the same within a run (`<file-3>.mkv`, `<dir-2>`,
  `<tag-4>`, `<user>`, `<md5-1>`, `<meta-1>`), so it is safe to send. The Logs panel's spy button shows every log that
  way (the job logs too) and copies it so. Settings > Advanced & Logs > Shareable log switches the file and each kind
  (all on by default). `config.toml` gets `[advanced] shareable_log` and `[advanced.redact]`, `ui-state.toml` the
  panel's choice; older files load with these defaults.

- Settings > Diagnostics (0.0.54, your UI review note 28#1, like RustView's Debug page): two cards, Dependencies
  (ffmpeg, ffprobe, mpv, vid.stab, the encoders that work, RustView's program) and Health (config.toml and
  ui-state.toml, the queue database, the folders RustCut writes, the target folders, the free space for renders and
  the cache, RustView's connection and library). What needs attention comes first, with the commands that fix it and a
  copy button; the checks that passed fold into one row (remembered). "Check again" runs them again, "Copy report"
  copies all of them for a message. Nothing is written while checking.

- Auto-crop also finds borders of any color (0.0.52, your UI review note 13#1): in the Crop filter's menu, "Border"
  has a pipette; click it, then a border in the video, and auto-crop looks for that color (white, gray, blue bars)
  instead of black. "Color tolerance" says how close a pixel must be; × goes back to dark borders. The color belongs to
  the video (kept with "keep"), never to the defaults.
- Auto-crop tries again when it finds nothing (0.0.52, your note 13#1): "Retries" (2 by default) in the Crop menu and
  Settings > Filters > Auto-crop, each try with more tolerance ("Raise by", 50 %), the last one at the maximum ("Most
  black" 64, "Most color tolerance" 30 %). Dark gray (#303030) bars are now found without changing anything. The
  message says which try found them. `config.toml` gets the new `[filters.crop_search]` values; older files load with
  these defaults.

- The crop's size can stay divisible by 2, 4, 8 or 16 (0.0.51, your UI review note 13): the Crop filter's menu and
  Settings > Filters > Crop have "Divisible by" (2 by default, what every encoder needs). Dragging the crop, the ratio
  button and auto-crop keep its width and height multiples of it; typed edges stay exact. Choosing a number fits the
  current crop at once (it only gets smaller). While a Resize after the crop sets the output size, the crop is free.
- One bubble on the Filters card's header says when the output size after every filter does not suit the profile's
  codec (0.0.51, your filter questionnaire): an odd size (the render makes it even), or a size that is no multiple of
  the codec's blocks (H.264 16, HEVC and AV1 8; checked in the encoders' headers), with the nearest size that fits. It
  never shows for the video's own size, only when the filters change it. `config.toml` gets `[filters] crop_multiple`
  (2 for older files) and `ui-state.toml` keeps it with the filters.

- The crop has a ratio button like RustView's crop tool (0.0.50, your UI review note 13): while the crop is edited on
  the video, the pill at its top right shows the ratio; a left click goes through the landscape ratios, a right click
  through the portrait ones (square ones are in both; O is the video's own). Each ratio keeps the area of the crop you
  made and the edges it touches, so going through the ratios and back gives the same crop. It reads "Free" once you
  drag the crop out of that shape. A double click inside the crop keeps it, like Enter.
- Settings > Filters > Crop ratios (0.0.50): the ratios as chips, as in RustView: add one ("16:9", "2.39:1" or "1.85"),
  remove one (held), or go back to the default list (held). Resize's ratio button offers the same list (your note
  13#4). `config.toml` gets `[filters] crop_ratios`; older files load with RustView's default list.

- Every filter but Orientation has an options menu (0.0.49, your filter questionnaire): the adjustments button at the
  end of its row opens a bubble beside it, with an arrow to the button, the options in groups and every change at
  once. Deinterlace: the deinterlacer, "Output" (frames or fields), "Field order", "Only interlaced frames", and what
  the interlace check counted with "Analyze Again". Crop: how auto-crop searches ("Black up to", "Search", "Sample
  points", "Only inside the sections"). Deshake: every value, each labeled by a small symbol whose tooltip names and
  explains it. Resize: the scaler. The menu's header jumps to Settings > Filters, pins the menu (it then stays open
  while you work on the video or in other panels; remembered across restarts), saves the options as the defaults for
  every new video, and resets them to those defaults (held; both are off while the options are the defaults). A click
  outside, Escape, × or the button again closes it.
- Filters move by drag & drop (0.0.49, your UI review note 12#1): drag a row by its grip or its free space; the other
  rows make room, Escape cancels. A right click on a row offers Switch On/Off, Options, Move Up, Move Down and Reset
  Options (held).

- Settings > Filters has new defaults (0.0.48, your filter questionnaire): Deinterlace "Output" (frames or fields),
  "Field order" (auto, top or bottom first) and "Only interlaced frames"; a new "Auto-crop" card with "Black up to"
  (how bright a border may be: 24 finds black bars, around 50 also dark gray ones), "Search" (all edges, top and
  bottom, or the sides), "Sample points" and "Only inside the sections" (on); Deshake "Max shift", "Max angle" and
  "Zoom speed" (for the adaptive zoom); a "Resize" card with the scaler. Each was checked against ffmpeg's manual and
  tried with the real ffmpeg. The Filters card's own menus for them follow in the next versions; older settings files
  load with these defaults.

- A play or pause symbol flashes in the middle of the video when a key plays or pauses it (0.0.47, your UI review round
  1 and your answers): a dark translucent disc with the white symbol that fades in, stands briefly and fades out
  (0.6 s), for the play/pause key and J, K and L (not for the menu or the button under the video, not at the end of a
  video or when one opens). While the video is paused a faint pause symbol stays in its middle. Settings > Player >
  "Play and pause" switches either off (both are on; older settings files load unchanged).

### Changed

- The size hint on the Filters card's header also comes for an odd size when no filter changes it, the video's own size
  too (0.0.58, your night questionnaire): the render cuts the last pixel row or column off to make it even. Whether
  the hint should still speak about multiples of 8 or 16 waits for the next questionnaire.
- The × that closes a filter's options menu, and the × of every dialog, is flat (0.0.57, your night questionnaire): no
  frame like the buttons beside it, only a light tint under the pointer.
- A filter row is lightly tinted while the pointer is anywhere on it, its buttons and fields too (0.0.57); "Switch On"
  and "Switch Off" in its right-click menu have a symbol like the other entries.
- A pinned options menu follows its filter when the card stack scrolls (0.0.57); while the filter is out of view the
  menu waits at the stack's edge.
- Settings > Messages: the three tabs stand over a surface of their own, and the selected tab merges into it like the
  tabs of the main window (0.0.57).
- The button of the Tag & Rate panel's RustView card leads to the RustView check of Settings > Diagnostics while
  RustView cannot be reached (0.0.54, your note 28#1); its tooltip still says the error.

- Every tab bar looks like a browser's (0.0.53, your UI review note 01#2 and your tab questionnaire): the selected tab
  is in its panel's color with rounded top corners and small curves into the panel, so it merges with it; the other
  tabs lie flat on the darker bar with a thin line between them and a light tint under the pointer. When the tabs do
  not fit, only the selected one keeps its title and the others show their symbols (the tooltip names them); when
  even that is too wide, a "+n" button lists the rest. A dragged tab lifts and follows the pointer while the others
  move aside. The same tabs are in the dialogs (counts as small badges: "Made from it 10") and in Settings > Messages.

- Dragging the crop on the video is free; hold Shift to keep its ratio (0.0.50, your note 13): the chosen ratio while
  the crop has it, else the crop's own. The aspect-ratio choice in the Filters card is gone (your note 13#2); the
  ratio you chose is remembered, and the one an older version kept (`crop_aspect` in `ui-state.toml`) is taken over as
  `crop_ratio`.

- The Filters card is rebuilt (0.0.49, layout A of your filter questionnaire): one row per filter with its name as the
  on/off button (filled and bold when on), its values always visible (dimmed while it is off; changing them keeps it
  off), and rows of fixed heights, so nothing jumps when a filter switches. Crop has two lines: its tools with the
  size it keeps, then its four edges. Deshake shows its main values in one dim line. The output size, the preview,
  keep and reset sit in the card's header; reset has a backspace symbol instead of the one that looked like reload.
  Resize shows what an empty side comes to as a dim number instead of "auto" (your note 13#3), and its 16:9 and 4:3
  helpers sit in a small menu of its ratio button. The move arrows are gone (drag & drop and the row's menu).
- The tooltips of the filter options say what is recommended; Settings > Filters and the menus share them (0.0.49).
  `ui-state.toml` remembers whether the menus are pinned (`[export] filter_menu_pinned`; older files load with it off).

- Rotate and Mirror are one filter, Orientation (0.0.48, your UI review round 1 and your answers): turn right or left
  (one or none, like radio buttons; a second click takes the turn back) and mirror left to right and top to bottom
  (each a switch of its own; both together turn the picture upside down). Pressed buttons show what is on, and a
  small "F" shows how the picture ends up (its tooltip says it in words). Every button and the keys R, Shift+R and the
  mirror keys switch Orientation on. Saved jobs and kept filters of older versions load with their rotation and mirror
  in one Orientation and render exactly as before.
- Changing a value of a filter that is off keeps it off; the value waits until you switch the filter on (0.0.48, your
  answer). Auto-crop, editing the crop on the video and the Orientation buttons and keys switch their filter on.
- Auto-crop looks only inside the sections when there are any (0.0.48), and new videos resize with lanczos, the
  sharpest scaler for shrinking.

- The player without a video shows a faint, colorless RustCut symbol with "Open a video with File > Open (Ctrl+O)" and
  "or drop it here." on two lines; the title and the version are gone (0.0.47).
- A collapsed sidebar's icons show their panel over the video (0.0.47, your answers): hovering an icon for 0.3 s or a
  click shows the panel's group at the sidebar's width over the full height, sliding in from the rail; it goes away
  0.4 s after the pointer left the rail and the group. Holding an icon (it fills in the accent color) keeps the group
  until you click that icon or hold it again. The rail's top button still expands the sidebar.
- RustCut has its own menu bar, your UI review round 1 and your answers on it (0.0.46): 32 px high instead of 40, every
  title and the bell vertically centered, the open title and its menu one piece in the menu's color. "Reset Layout"
  (View), "Reset Filters" (Filters) and the new "Clear All Sections" (Sections) act only when held (the entry fills
  while it is held; its tooltip shows the hold badge); their keys act at once as before. With a menu open, Up and Down
  move through the entries, Left and Right go to the neighbor menus, Enter acts and Escape closes; moving the pointer
  over another title switches to its menu. A held entry in any menu now acts when you let go after the fill, so the
  next click is no longer lost. The sidebars start right under the menu bar, on the line of the video's top edge.
- No shadows anywhere (0.0.46): menus, choice lists, flyouts, dialogs, toasts, the bell list, bubbles, tooltips and
  dragged tabs are flat with their border.
- The Job panel, the Info panel and the Logs panel, your UI review round 1 (0.0.45): the job's state is a badge in its
  color, the render time has its own row under "Ended", and the RustView row starts with a symbol (written, failed,
  waiting, not written, removed). The source's file row has buttons on its right that play the video in the default
  player and show it in the file manager, its folder row one that opens the folder; the output has the same once it
  is rendered. A profile the library still holds unchanged gets a button that shows it in Settings > Render Profiles
  (selected, its row flashes). The source and the codecs have one row per attribute (size, frame rate, duration;
  video codec, quality, preset, bit depth, audio codec, bit rate, channels, sample rate, container), the attributes
  indented under their codec. The Media Info tab is called "Info", and each of its cards folds with a click on its
  title (for this run). The Logs panel shows no line numbers.
- The Codecs panel, your UI review round 1 (0.0.44): the profile choice takes the whole row up to its buttons, and the
  audio bit rate is a slider over the bit rates the audio codec offers (Opus 48 to 512 kbit/s, AAC, MP3 and Vorbis
  64 to 320 kbit/s), with the value under it and "(default)" at the codec's default; it is hidden for FLAC and copied
  audio as before. The same slider is in Settings > Render Profiles. A profile that names no bit rate keeps the
  codec's default, and a bit rate the slider does not offer (written by hand) stands at the nearest step and says
  "outside the slider" until the slider is moved.
- The Tag & Rate panel, your UI review round 1 (0.0.43): without an open video (and while no queued job's tags are
  edited) the rating, tagging in the tree, the history's tags and the last rating and tags are dimmed and locked;
  filtering, opening and folding branches, folding the history, clearing, the switches, the eye and the tag filter
  still work. A locked tag (right click) is red instead of green, without the lock symbol that pushed its name aside.
  The row under the history has no text between its buttons any more. The eye only shows or hides the hidden tags
  (dimmed in the tree, and a click assigns them as usual); which tags are hidden is chosen in the tag filter only, so
  the eye's own mode with its hint is gone. Active's tooltip says "When the job is saved, its tags and rating are
  written into RustView for the target file." The tag filter window is called "Tags Filter", says "Select the tags to
  hide", and a check now means hidden; its changes wait for "Apply" (it replaces "Done"), while Escape or the window's
  × close it without changing anything. While RustView is unavailable or switched off, its card says so in one short
  line under a larger symbol; the button to the RustView settings names the error in its tooltip. The tag filter field
  is off while there are no tags to filter. The buttons that expanded and collapsed every branch are gone.
  `ui-state.toml` keeps whether the eye shows the hidden tags (a new field; older files load unchanged).
- Job Finalization, your UI review round 1 (0.0.42): two rows whose three columns line up: the name (the chosen
  container's extension stands dim inside it, ".mkv"), the counter and the auto-increment toggle; the target folder,
  a button to the folder list in Settings > Export & Naming (it replaces adding and removing folders on the card) and
  Save. The field icons, the file-name line and the counter's own switch are gone: an empty counter field means no
  counter (with auto-increment it stays empty), a number switches it on; Save's tooltip names the file.
- Settings > Export & Naming > Target folders (your wishes with the review): each folder's path is a field you can
  type into. The path is checked against what Linux allows (it starts at /, ~ stands for your home folder, a folder
  name has at most 255 bytes, the path at most 4096); a refused path keeps the field open with a bubble that says
  why; a path that is a file is refused the same way and the old path stays. A label that was the old folder's name
  follows the new one. A folder that does not exist yet is fine: a sign inside its path field says so, and it is made
  on the fly when a job writes into it (any job, also one saved before the folder went missing; the export card no
  longer refuses a missing folder). Each row has a button that picks the folder with the file dialog and a handle to
  drag the row to another place (the page scrolls along at its edges; Escape cancels).
- The queue panel, your UI review round 1 (0.0.41): the search and the state chips moved into a filter menu that
  hangs from the filter button at the right of the top bar: the search on top, a switch for how the filters combine
  (ALL: a job matches every filter that is set, the default; ANY: one is enough), and one choice each for the states,
  the target folders and the profiles (a click picks one value, Ctrl+click several; each entry shows how many jobs it
  has). The button's badge counts the filters that are set, a right click on it shows every job again, and a click
  outside or Escape closes the menu (the filters stay; they are kept across restarts). The queue's summary and the
  overall progress moved under the list. "When the queue finishes…" hangs from its button with colored entries ("Do
  Nothing", "Send Notification", "Quit App", "Suspend Device", "Power Off Device") and the check mark on the right; a
  dot in the entry's color sits on the button while one is chosen, and a right click resets it to "Do Nothing". The
  job menu has "Move Up", "Move Down", "Render Again (Hold)" (renders again, held first), "Select All" and "Clear
  Selection"; a right click below the jobs offers "Select All", "Show Every Job" and "Remove Finished Jobs", a click
  there clears the selection. The hand's tooltip says "Hold job" / "Hold 3 jobs" / "Release job". In the two-line rows
  the percentage of a render stands on the second row, on the right; the table has a held delete button per job and
  the column "Time" (the time left while a job renders, afterwards how long its render took). The pencil on the row of
  a loaded job is gone (its highlight stays).
- Context menus open exactly at the pointer (they opened a menu bar's height too low).
- The Sections card and the timeline, your UI review round 1 (0.0.40): the current section is one row (set the start
  to the playhead, its start, its end, set the end, add it; both times share the width equally, the length is in
  their tooltips and in the Add button's). Sections are selected like jobs in the queue (Ctrl adds or removes one,
  Shift a range) and have a context menu: Duplicate (the copies go after the last selected section and flash),
  Move Up, Move Down, Delete (held), Select All and Clear Selection; a right click below the rows offers Select All
  and Clear All Sections (held), a click there clears the selection. Every row has its own delete button (held), the
  durations use the compact form ("14.840s"), and the rows are dragged to another place like the queue's jobs (the
  Export panel scrolls along near its edges; Escape cancels). The buttons under the list are gone (the menu, the
  keys and the drag do it), and "Clear the sections after each save" moved from Job Finalization into the Sections
  header. On the timeline the start and end marks are flags, a selected section is outlined, the section under the
  pointer is brighter, and a section's body can be dragged to move it whole (its start or end snaps like an edge; a
  click on it still selects it and seeks there). While an edge or a section is dragged, its start, end and length
  stand over it on the ruler, and Ctrl moves a tenth as far for fine work (Shift still snaps to frames only). Edges
  and sections snap to where the playhead was when the drag started.
- Dragging jobs in the queue is a real drag (0.0.39, your UI review): the selected jobs lift off as one
  semi-transparent ghost with a dashed outline that follows the pointer ("2 jobs" and a "+1" badge when several move),
  the other jobs slide aside to open a gap where they will land, and near the top or bottom edge the list scrolls
  along, faster the closer the pointer gets to the edge. Escape cancels the drag; tooltips stay hidden while you drag;
  releasing a drag no longer changes the selection. The same drag comes to the sections and the filters later.
- Dialogs, your UI review round 1 (0.0.38): every dialog's symbol sits vertically centered beside its title, and
  every dialog you can cancel has an × in the top right corner (Escape does the same; the start dialogs keep none).
  Radio marks and key numbers of the choices are centered on their rows, and the start dialogs mark the recommended
  choice with "(Recommended)" (no yellow choices any more). The file lists of the dialogs have subtle lines between
  the files, no number badges on the pictures, and the pictures and buttons are centered on their rows; the existing
  file's play and folder buttons sit on the right of its tile.
- The save dialog about the target says "Target file already exists"; "Next free number" says it takes the next
  number not used in the target folder; "Overwrite" is no longer highlighted; a typed name that is taken says
  "*name* already exists." (0.0.38). The files dialog of the name check opens on "Similar files" when nothing has the
  name yet and shows "Target (0)".
- The question about a job without tags or rating is compact (0.0.38): the title, the rating bar, "Save without" and
  "Back to editing", which is the main button and has the keyboard: Enter goes back without saving, Left and Right
  move a visible focus through the ratings and the buttons, 0-5 still save with that rating, Escape and × go back.
  "Don't ask again" is gone from the dialog; the setting stays in Settings > Warnings & Confirmations and as "Warn" on
  the Tag & Rate panel.
- Other dialog words (0.0.38): loading a job asks "Replace current session with Job #3?" ("The current session is
  not saved."); quitting while rendering says "A job is rendering. What to do?" with "Force quit" (held), "Quit when
  done" and "Cancel"; a settings file from a newer RustCut is a "Version conflict detected" that asks you to decide,
  says "There is 1 setting" or "There are 2 settings this version does not know" and lists them one per line.
- The color picker of the Settings window belongs to it (0.0.38): while the picker is open, the Settings window is
  dimmed and takes no input (a click brings the picker forward); changes still show at once.
- Messages, your UI review round 1 (0.0.37): toasts show their time as hh:mm:ss in the title row, left of ×. Banners
  show their symbol larger and centered, and the time their state began (hh:mm:ss, no "since") left of ×; their
  buttons and the pager ("‹ 1 / 2 ›") stand left of the time, everything centered in height. The bell keeps its
  color; only its number shows the color of the most serious unread message. In bubbles the arrow joins the border
  without a gap, and the link and the buttons share one row: the link on the left, the buttons on the right (buttons
  alone stand on the left).
- The name check's bubbles say it shorter (0.0.37): "Target filename already exists" with "Show the file", "Job #2
  already uses the target filename", "3 similar filenames in target folder" (without the list of names), and "If you
  do not fix it here, Save opens a dialog that helps you resolve it."; the number button's tooltip is just "Take the
  next free number: 03".
- The strip "Job #n loaded" in the Job Finalization card is gone (0.0.37): loading a job is the message "Job loaded
  into the editor" in Settings > Messages, off by default (the loaded job is highlighted in the queue); as a toast it
  offers "Show the job" and "Unlink". The kind that was called "Job loaded into the editor" before is now "Other
  notes about loaded jobs" (its setting stays).
- Shared controls, your UI review round 1 (0.0.36): choices stretch up to one common width (the Quality slider too),
  cut a long choice with "…" and open a list as wide as its longest entry. Number fields have small up and down arrows
  (Shift: ten steps), show their unit inside the field ("px", "s", "frames", "kbit/s") and step with the mouse wheel
  only after a click into the field, so scrolling a panel never changes a value; Up and Down step while you type.
  Each field has its own step (Resize 16 px, Deshake smoothing 5 frames). The Settings window uses the same widths,
  so its choices and numbers line up.
- Names and paths are cut in the middle when they do not fit, so the end stays readable (0.0.36): "basic_h264… -
  03.mkv" in the queue, the Job and Logs panels, the target folders in Settings and the dialogs about the target file.
- The Sections card's header shows the number of sections and their total length on the right, "2 • 10.500s" (0.0.36).
  These and the Job panel's sections use a compact time form that never changes its width: always three decimals,
  minutes and seconds always two digits ("05.500s", "02:33.100", "3:02:33.000").
- Buttons and links that lead somewhere in RustCut show the jump symbol (0.0.36): "Show the option", "Show the
  sections" and "Show log" in messages, the links in bubbles, the settings button of the RustView card.

- Loading a job into the editor always makes a copy (0.0.35, your decision): the edit button of the queue and of the
  Job panel loads waiting, held, finished, failed and canceled jobs alike, and Save makes a new job; the loaded job
  stays as it is (it is no longer held while it is loaded and never updated). If the new job replaces it, remove or
  hold the old one yourself (the name check says when another job writes the same file). The card's banner names the
  loaded job; "Save as new" is gone, the context menu has one "Load into the Editor".
- The queue's pause is saved at once (0.0.35): a queue paused before RustCut crashed or the computer went off stays
  paused at the next start (before, only a normal quit saved it).
- The Tag & Rate panel's layout follows PyCutRv (0.0.33): the rating bar on top, expand and collapse beside the tag
  filter, and one row under the tag history with the last rating, the last tags, clearing, Active, Warn and hiding
  branches. The line "RustView connected" is gone: while RustView is unavailable or switched off, its card (the
  reason, the settings button and Retry) takes the tag tree's place, and the reason is no longer said twice.
- **Settings:** "Ask when the job has no tags" and "Ask when the job has no rating" are one switch now, "Warn about
  missing tags or rating" (`warnings.missing_metadata`, the panel's Warn), on when either was on (0.0.33). The
  settings file is migrated to version 4 (`one-metadata-warning`); the old file is backed up first. "Don't ask again"
  in the question switches Warn off.
- New defaults like PyCutRv (0.0.31): a gray background around the video (#444444: a video's own black borders stand
  out), the queue starts paused when jobs are waiting, and Save without sections takes the marked range or the whole
  video. A `config.toml` that already holds these settings keeps its values (change them in the Settings window).
- A job loaded into the editor opens at the start of its first section (0.0.31, like PyCutRv).
- The hint about similar names in the target folder, and the similar files in the dialogs, no longer list the video
  being cut (0.0.31).

- Cards that need a video (Job Finalization, Filters, Sections, Probe Render) are dimmed and do not react while no
  video is open, instead of saying so in a message (0.0.29); their keys do nothing then. The message kind "Needs an
  open video" is gone.
- The job loaded into the editor is highlighted in the queue (an amber frame) until the editing ends (0.0.29).
- The Tag & Rate panel shows RustView's state as a small card while RustView is switched off or unavailable: its gray
  symbol, the reason below and a button to the RustView settings (0.0.29); connected, it stays one slim line.
- Loading a job into the editor knows two ways only (0.0.29): replace the open video, or ask first. "Set it aside"
  and the word "draft" are gone; the question has "Cancel" and "Replace". **Migration:** `config.toml` goes from
  version 2 to 3 (backed up first): `[editor] draft_on_load` is now `on_job_load`, and a stored `set_aside` reads as
  `ask`.

- The line under the file name in Job Finalization, the issue list of the Codecs panel's Profile card, the red text
  under a refused Settings value and the overlap note under the section list became bubbles at their fields (0.0.27).
  A refused typed section time no longer shows a toast.

- The desktop notification switches (Desktop Integration) and the warning about a source cut before (Warnings &
  Confirmations) moved to Settings > Messages; their old places have a button that opens it (0.0.26).
- The Hotkeys page's search field got a clear button (the same field as on the Messages page).

- Messages appear where their kind belongs (0.0.25) instead of all in the status line, which keeps only quiet
  confirmations ("Section 2 added", "Filters reset", "Ready"). Loading or leaving a job in the editor says nothing
  unless it fails; a finished job, by default, says nothing either (the queue shows it).
- The countdown after the queue (quit, suspend, power off) is a desktop notification with Abort by default; it can
  be a toast or a banner instead (Escape still aborts).
- A file that does not play shows the reason in the banner over the video; the video area keeps only the big Reload
  button.
- **Settings migration (config.toml version 1 to 2, backed up first):** the desktop notification switches and their
  time moved from `[desktop.notifications]` to `[notices.desktop]` and `[notices.kinds.<kind>] desktop`, and
  `warnings.known_source = false` became `[notices.kinds.known_source] place = "nothing"`. The Settings window shows
  them where they were for now. An older RustCut reads the new file as "from a newer version" and does not save it.

- The Job Finalization card checks the job again at once when the Codecs panel, the sections or the source's
  description change; an error of the render settings turns Save off with the reason in its tooltip instead of
  failing at the click (0.0.23).
- Buttons and menu entries that must be held show a red "Hold 2 s" badge in their tooltip (the time follows the
  setting) instead of a text starting with "Hold to" (0.0.21).
- The details behind the "i" of the "file exists" dialog use the new tooltip (0.0.21).
- The next free number passes over every number the name's series uses in another video format: with "Clip - 02.mp4"
  in the folder, an MKV job never becomes "Clip - 02.mkv". Gaps are filled first (0.0.20).
- The dialog's Save button names no file ("Save", "Overwrite", "Remove and save"); the chosen row shows the name. The
  header shows the whole path; a long folder is shortened, never the file name. "Don't ask again" left the dialog:
  whether it asks is on the Warnings page.
- Less drive work: pictures read only the keyframe they show (0.2-2 MB per file instead of up to the whole file), the
  pictures' ffprobe and ffmpeg and the MD5 reads run at a low priority so playback comes first, MD5 reads ask the
  kernel for larger read-ahead, and folders are read in one pass of names only.

- Arch package (0.0.19): `./build-package.sh` builds `dist/rustcut-<version>-1-x86_64.pkg.tar.zst` (install it with
  `sudo pacman -U dist/rustcut-<version>-1-x86_64.pkg.tar.zst`). It brings the desktop entry (RustCut opens videos
  from Dolphin and other file managers, several at once; the taskbar entry shows the queue's progress), the AppStream
  metainfo and the app icon in every size; it depends on `mpv` and `ffmpeg`, RustView and the desktop portal are
  optional. The package is built without the debugging server for automated UI checks.
- `scripts/release.sh` prepares a release (package, checksums, notes from this changelog) without tagging or publishing
  anything; `scripts/check-all.sh` validates the desktop file and the metainfo and, without `--fast`, builds the package,
  checks its content and lints it with namcap.
- App icon (0.0.18): a film frame with scissors in RustView's style; the windows and the tray use it.
- Tray icon (optional, `desktop.tray.enabled`, off by default): shows the queue in its tooltip ("1 rendering, 2 waiting ·
  42 %"); a click shows or hides the window (a minimized window comes back), its menu pauses or resumes the queue
  and quits. With `desktop.tray.minimize_to_tray`, minimizing the window while a job renders hides it in the tray; the
  open video plays on from the same place when the window comes back.
- Notifications for finished jobs (with "Open folder"; off by default), failed jobs (with the reason and "Show log")
  and the queue being done (`desktop.notifications.*`).
- When the queue is done (0.0.17): a power button in the queue panel chooses what happens after the last job:
  nothing, a notification, quit RustCut, suspend or power off. Before quit, suspend or power off a banner above the
  video counts down (`queue.when_done_countdown_secs`, default 5 s) with an Abort button (Escape also aborts), and a
  desktop notification with Abort appears. After quitting, suspending or powering off, the choice goes back to
  "nothing", so it never repeats by surprise; the choice is kept in `ui-state.toml`.
- Closing RustCut while a job renders asks first (`general.confirm_quit_while_rendering`): keep rendering (Escape),
  quit when the queue is done, or quit now (hold the button; the job starts again at the next start).
- While a job renders, the computer does not go to sleep (`desktop.inhibit_sleep`, through systemd-logind).
- The queue's progress and the number of open jobs show on RustCut's taskbar entry (`desktop.taskbar_progress`; KDE
  Plasma shows it once RustCut's desktop file is installed).
- Logs panel (0.0.16): besides the log of the job chosen in the queue, it shows RustCut's own log of this run (the
  lines the log file gets, with time and level) or only its RustView part (the link and the Tag & Rate panel). The
  log grows while it is shown; search, "errors only", following the newest line and Copy work on every log. The
  chosen log is kept in `ui-state.toml` (`logs.source`).
- Settings window (0.0.15; File > Settings..., Ctrl+, or Help > About RustCut), styled like RustView's: General, Player,
  Editor & Sections, Filters, Export & Naming, Render Profiles, Queue, RustView, Warnings & Confirmations, Hotkeys,
  Desktop Integration, Appearance, Advanced & Logs and About (with the Slint attribution). Every change is saved at
  once (no Save button); options that only work from the next start say so. A held button in the page header puts
  the page's options back to their defaults. The window's size, its page, the selected profile and the hotkey filter
  are kept in `ui-state.toml` (new `[settings]` table).
  - Export & Naming: the file-name template is checked while you type it and shows an example; the named target
    folders can be added, labeled, reordered and removed (held).
  - Render Profiles: every profile with the one new jobs start with (star); built-in profiles are shown read-only,
    "Duplicate" makes an own copy, and an own profile is saved with every change, renamed or deleted (held). The
    Codecs panel follows at once.
  - Hotkeys: every action with its keys, a filter, a key button that waits for the next key (Escape cancels), a
    cross per key, and a reset per action. A key another action has is named first; pressing it again moves it.
    `config.toml` keeps only the actions whose keys differ from the defaults.
- New settings (no migration; older files load with these defaults): `appearance.tooltips` (on) and
  `appearance.tooltip_delay_ms` (600), `rustview.tag_history_size` (20, up to 100), key `open_settings` (Ctrl+,).
- Help menu: open the settings, data and log folders.

- Queue panel (0.0.13): in a narrow sidebar the jobs show as two-line rows; as the panel gets wider (or its group is
  maximized) it becomes a table whose columns join as there is room: number, name, state with the render's
  progress, ETA, length (and the number of sections), size (estimated while rendering, measured when finished),
  profile and folder. Drag a column's edge to make it wider or narrower. Select several jobs with Ctrl+click and
  Shift+click (Ctrl+A selects every job shown); drag the selected jobs to another place, or move them with
  Ctrl+Up/Down and Ctrl+Home/End (configurable keys `queue_move_up`, `queue_move_down`, `queue_move_top`,
  `queue_move_bottom`, `queue_select_all`). State chips with counts (waiting, running, held, finished, failed,
  canceled) and a search over names, paths, profile, note and failure narrow the list. The header shows the overall
  progress of the current run with the time left, and a second switch pauses after the running jobs. A double click
  plays a finished job's output, shows a failed or canceled job's log, and shows any other job in the Job panel.
  The filter, the search, the column widths and the selection are kept in `ui-state.toml`.
- A context menu for the selected jobs (right click), with the number of jobs each entry reaches: edit in the editor
  or load a done job as a new one, show in the Job panel, show the log, edit tags and rating, play the output, show
  it in its folder, hold or release, render again, **render again with another profile** (any profile, or the Codecs
  panel's current settings), duplicate, move to the top or bottom, cancel, remove, and remove every finished job.
- Job panel: a new dock panel (next to Codecs by default, movable like every panel) shows the queue's current job:
  its state, times, attempt and RustView state, the source, the output, every section, the filters in their order,
  the codec settings, and the tags (by name) and rating, in sections you can fold. Its buttons edit, play, reveal,
  show the log, edit the tags, render again and duplicate the job.
- Logs panel: the log of the queue's current job (RustCut's notes and ffmpeg's lines; errors red, warnings yellow),
  with a search, "errors and warnings only", following the newest line and Copy to the clipboard. The log of a
  running render is stored when it ends.
- Editing queued jobs: "Edit in the editor" loads a waiting job's video, sections, filters, codec settings, target
  and tags into the editor; the job is held meanwhile and waits again afterwards. A banner over the Job Finalization
  card says which job is edited. Save updates the job while it writes the same file; another name, counter number,
  folder or container makes a new job instead, and "Save as new" always does. A finished, failed or canceled job can
  be loaded as the start of a new job. The new setting `editor.draft_on_load` decides what happens to the editor's
  draft: `replace` (default, like PyCutRv), `ask`, or `set_aside` (it comes back when the editing ends). Jobs now
  remember the name and counter they were saved with, so they show the same in the editor (jobs saved by older
  versions show their file name).
- Hold to confirm: buttons and menu entries that delete or discard something (remove or cancel jobs, remove the
  finished jobs, remove a target folder, delete or reset a render profile, delete a section, reset the filters,
  clear the tags) act only while held, instead of asking in a dialog: a red line runs around the button and its
  background fills up; let go early and nothing happens (a short click wiggles it and shows what holding does). The
  time is the new setting `general.hold_to_confirm_ms` (default 2000; 0 = a click is enough). Their keys still act
  at once.

- Tag & Rate panel (0.0.12): RustView's tag tree (in columns, with the same look as in RustView), its rating bar, the
  tag filter (plain text or, with the `.*` switch, a regular expression; an invalid expression is matched as text and
  says so) and the tag history (recently used tags, a click assigns or removes one). The tags and the rating you
  choose go with the next saved job into RustView. After a save the panel is cleared; "Apply the last saved job's
  tags" and the last rating button bring them back with one click. **Right-click a tag to lock it:** a locked tag
  (lock icon) stays assigned for every following job until you right-click it again. "Clear" removes the tags and
  the rating (locked tags stay); expand/collapse every branch; the eye button lets you hide branches you never use
  (click a branch to hide or show it, hidden ones are dimmed while choosing). The panel's header is the one place
  that says whether RustView is connected; when it cannot be reached it shows why, retries by itself and offers
  Retry. Expanded branches, filter, history, hidden branches, locked tags, the history's height and the last tags
  and rating are kept in `ui-state.toml` (new `[tags]` table; older files load unchanged, no migration).
- Saving a job without tags or without a rating asks once (`warnings.no_tags`, `warnings.no_rating`): the dialog
  names what is missing, a click on a rating (or the keys 0-5) saves with it, "Save without" (Enter) saves as it is,
  "Back to tagging" (Escape) shows the Tag & Rate panel, and "Don't ask again" switches the asking warning off. It
  asks about tags only when RustView offers tags, and nothing while the RustView link is switched off.
- Editing a job's RustView entry: the tag button in the queue panel switches the Tag & Rate panel to the selected
  job ("Editing: <name>"); Apply writes the new tags and rating (a waiting job's record is updated, a finished job's
  file record in RustView too; a failed or canceled job keeps them for a retry), Cancel or Escape returns to the next
  job's tags, which are kept meanwhile.
- RustView link (0.0.11): RustCut now talks to RustView through `rustview api` in the background (never blocking the
  window; one call at a time, each with the time limit `rustview.timeout_secs`, the program from `rustview.binary` or
  `rustview` in your PATH). Each queued job gets its record in the RustView library according to
  `rustview.write_timing`: "at_enqueue" (default) writes a pending record when the job is saved, which RustView
  completes once the file exists, and writes it once more after the render was verified; "after_render" writes it
  only after the render. Before the first write RustCut asks whether RustView already has a record for the file: such
  a record keeps its source app and is never deleted by RustCut; only the values RustCut changed are given back when
  the job does not produce the file. A job that fails, is canceled or removed before it finished loses the pending
  record RustCut created; a waiting job whose file name changes moves its record along. A record stays when the file
  exists or when RustView now says another app made it. While RustView cannot be reached, RustCut tries again after
  2 s, then 4 s, 8 s, ... up to every 2 minutes; nothing is lost when RustCut quits meanwhile (the next start
  continues). The tags and rating come from the Tag & Rate panel (0.0.12).
- Known-source warning: when a video opens, its MD5 comes from RustView's record of the file if the record matches
  the file's size and time (shown as "From the RustView library" in Media Info, no need to read the file), else from
  the sidecar `.<name>.md5` or by reading the file. RustView is then asked whether a job was made from this video
  before; the banner over the video shows when (UTC) and the files made from it (the video's own library record does
  not count). The source's MD5 is registered in RustView only when a job of it is saved, never just by opening it.
  Switch the warning off with `warnings.known_source = false`.
- Saving jobs (0.0.10): Save (Ctrl+S or F9, also File > Save Job) turns the open video into a job of the render
  queue: its sections (with an empty list the automatic section of `editor.auto_section`, else Save asks for a
  section), the filter chain in its order, a copy of the Render Settings panel's current settings (changing them later
  does not change the job) and what the probe found about the source.
- Job Finalization card (Export panel): the target folder ("Source folder" = next to the video, always first, then
  your named folders; the folder icons add a folder through the system's folder dialog or take the chosen one off the
  list, the folder itself stays; stored in `config.toml`, `[[export.target_folders]]`), the name (the video's file
  name when a video opens), the counter with its switch, auto-increment (the counter goes up after each save) and
  "clear the sections after each save" (both kept in `ui-state.toml`). Below them the file the job will write
  (naming template `export.name_template`) and a live check: free, a hint when other files in the folder start with
  the same name, a conflict that Save will ask about (the file exists, or a job in the queue writes it), or why the
  job cannot be saved (no video, the name is the source file itself, the folder cannot be read). Opening another
  video starts the counter at 1 again.
- Conflict dialog when saving: when the file exists or a queued job writes the same file, a dialog shows the
  existing file (size and date, play it or show it in the file manager), the queued jobs, similar names in the folder,
  and every way out with the file name it leads to: the next free number (the counter continues after it for the
  next job; "Add a number" when the counter is off), "Name (n)", another name typed in the dialog (checked while you
  type), overwrite (the old file is replaced only after the new render is verified), remove the queued jobs, or save
  anyway. Keys 1-6 pick, Up/Down move, Enter saves, Escape cancels. "Don't ask again" switches the warning off in
  `config.toml` (`[warnings]` `target_exists`: then a free name is taken without asking, an existing file is never
  replaced unasked; `target_queued`: then the job is saved anyway).
- Render queue: saved jobs render one after the other (`queue.workers` renders at the same time, default 1) at low
  priority; two jobs that write the same file never render at the same time, and a job that cannot render fails on
  its own without holding up the jobs behind it. The queue is stored in the profile's database
  (`~/.local/share/rustcut/queue.sqlite`) with each render's log, and comes back after a restart; a render stopped by
  quitting, a crash or a kill waits again at the next start (with a note), and what it left (the hidden temp file, the
  deshake and two-pass work files) is deleted. Removing a job never touches its source or the source's `.md5` file.
- Queue panel (a first version; the full panel follows): two-line rows with the file, the state, a progress bar,
  and the folder, profile and length, or while rendering the pass, ETA, the estimated file size, speed and fps, or
  why a job failed; the job id, attempt, target, source and notes in the tooltip. Buttons: pause/resume processing
  (paused renders stop where they are; kept across restarts), and for the selected job move up/down, hold/release,
  render again, cancel (an existing target stays) and remove.
- Render Settings panel (left sidebar, next to Export), in five cards:
  - Profile: quick selection of the built-in profiles and your own. Changes go to a working copy marked "(modified)";
    icons drop the changes, save them into your own profile, save them as a new profile, rename or delete your profile.
    Your profiles are stored in `config.toml` (`[[render.profiles]]`); the working copy and the profile it came from
    are remembered across restarts (`ui-state.toml`, `[export]`: `profile`, `render`). Problems of the settings are
    listed here, errors first (the render would fail or the file would not be standard) and then warnings (plays on few
    devices, slow combinations); the option concerned is marked in its card.
  - Encoding (the simple view): the codec (only encoders that work on this computer; the software encoders while the
    check at start still runs), a quality slider from "Small" over "Good", "High" and "Very high" to "Archive" that means
    about the same picture with every codec (the codec's own CRF, QP or CQ is shown under it), five speeds from
    "Fastest (largest files)" to "Slowest (smallest files)", and the container. Switching the codec keeps the slider
    position, the speed, a bit rate, the keyframe interval and the color tags; options only the old codec had are
    dropped, and H.264 starts at 8 bits.
  - Audio: codec, bit rate, channels and sample rate.
  - Expert (folded at first), per codec: rate control (constant quality, constant QP, average bit rate, bit rate in two
    passes, target size in two passes) with its value, the codec's full preset list, tune, profile, level, bit depth,
    keyframe interval, film grain and variance boost (SVT-AV1), AQ mode, keyframes at scene cuts (x264, x265), color
    tags and range (labels only, nothing is converted), and extra codec parameters (`key=value:key=value`).
  - Probe Render (see below).
  Every option has a tooltip that says what it does and what it costs.
- The real encoder checks the settings in the background half a second after a change (a few frames of a test picture
  with silence, into the chosen container): settings it refuses and codec parameters it ignores (x264, x265, SVT-AV1 and
  rav1e only warn about keys they do not know) are listed as errors, its own advice as warnings.
- Probe render (T, the flask button of the Probe Render card, or Filters > Probe Render): renders a few seconds around
  the playhead (10 by default, 2 to 60, a quarter of it before the playhead) with the current filters, deshake included,
  and the render settings at the encoder's fastest speed, then plays it in the player in a loop with a banner above the
  video. Escape or the banner's button returns to the source at the same place, paused or playing as before. While the
  probe plays, the timeline is dimmed and marks and sections wait. "Exact settings" renders with the profile's own
  speed and estimates the size of the whole job (the sections, or the whole video without sections). The length and
  the switch are remembered (`ui-state.toml`, `[export.probe]`). Probes are written to the cache folder (`probe/`) and
  removed at the next start.
- Size estimate of a render after its first seconds, from the bytes written so far (used by the probe render now and by
  the queue later).
- No migration: `config.toml` and `ui-state.toml` of 0.0.8 load unchanged (the new options have defaults), and a user
  profile in `config.toml` that cannot be read is skipped instead of resetting all settings.

- Render engine (not yet reachable from the window; the queue that drives it comes next): a job's sections are cut with
  frame-accurate seeking and joined, every audio track of the source is kept with its language and title, the filters
  run exactly as previewed (also on rotated phone videos and odd sizes, which are rounded to even pixels), and global
  metadata is copied; subtitles and chapters are left out. Variable frame rates (screen and phone recordings) keep
  their timing in every container. Deshake renders in two passes (motion analysis, then the transform); two-pass
  bit-rate profiles get their first pass. ffmpeg runs at a lower CPU and idle I/O priority (`render.nice`,
  `render.idle_io`), can be paused without using any CPU, and is stopped when it makes no progress for
  `render.stall_timeout_secs`. The output is written to a hidden `.<name>.rustcut-part.<ext>` file next to the target,
  checked with ffprobe (video, every audio track, size, duration) and only then renamed onto the target. A failed or
  canceled render removes only its temp file: an existing file at the target is never touched, and without
  "overwrite" a file that appeared meanwhile is never replaced. Each render keeps a log with the exact ffmpeg command
  lines and ffmpeg's messages; a failure names ffmpeg's actual reason.
- Encoder check at start, in the background (about half a second): every encoder RustCut knows is tried with a
  one-frame test encode, so hardware encoders (VAAPI, NVENC, QSV) are offered later only when they really work here; the
  result, with the reason for each encoder that does not work, is written to the log.

- Filters card in the Export panel: deinterlace, crop, deshake, rotate, mirror and resize, one row each with a switch,
  shown in the order they run (move a filter up or down to change it), their settings while switched on, the output size
  ("Output 1916×1076", or the even size the render makes of an odd one, or a warning when the crop removes everything),
  and buttons to show the filters in the player or not, to keep them for the next video and the next start (all filters
  with their order; `ui-state.toml`, `[export]`), and to reset them. Buttons are icons; their tooltips explain them and
  show their keys.
- Live preview: the player shows the filters as they will be rendered (deshake only in the render; it needs two passes).
  Rotated videos (phone recordings) are cropped in the orientation they are shown, like the render does.
- Crop: top, right, bottom and left as seen in the output (after a rotation or mirror that follows the crop); type a
  value or use the mouse wheel over a field (2 pixels per notch, Shift: 20).
- Cropping on the video: press C or the crop button to see the whole picture with the crop on it; drag a corner, an edge
  or the area itself, with the size shown, the rest dimmed and thirds lines while dragging; Enter keeps the crop, Escape
  restores the one from before. Quicker: hold Ctrl over the video and drag; the edge or corner nearest to the pointer
  follows, no aiming needed, and releasing Ctrl keeps the result. An aspect ratio lock (Free, Source, 16:9, 4:3, 3:2,
  1:1, 9:16) shapes the crop while it is dragged; the choice is kept. Edges snap to even pixels.
- Auto-crop (Shift+C or the wand button) looks at up to eight points of the video instead of only the current frame,
  ignores black scenes, and crops only what is black everywhere, rounded to even pixels.
- Deinterlace with bwdif, yadif, w3fdif or kerndeint, optionally one frame per field (2×); a background check of the
  video says next to it whether it is interlaced (and which field comes first). nnedi is not offered: it needs a weights
  file that ffmpeg does not ship.
- Resize to a width and/or height (an empty side keeps the aspect ratio, shown as "auto (720)"), with 16:9 and 4:3
  helpers that compute the height from the width and a choice of scaling algorithm; the output has square pixels.
- Rotate in quarter turns (R right, Shift+R left) and mirror left/right and top/bottom.
- Deshake settings: shakiness, accuracy, step size, smoothing, zoom, automatic zoom, border fill, interpolation and the
  virtual tripod.
- Filters menu: show or hide the filters in the player (P), keep filters, edit the crop on the video (C), detect black
  borders (Shift+C), rotate left/right, mirror, reset. All are keymap actions (`crop_edit`, `auto_crop`,
  `toggle_filter_preview`, `toggle_keep_filters`, `rotate_left`, `rotate_right`, `mirror_horizontal`,
  `mirror_vertical`, `reset_filters`).
- Tooltips on the buttons of the Filters and Sections cards, with their keys.

- Timeline under the video: a time ruler, every section of the list as a numbered block (the selected one highlighted,
  overlapping ones outlined in red), the current section's start and end marks with the range between them, the
  playhead, and the time under the mouse pointer. Click to jump there and drag to scrub; click a section to select it;
  drag a section edge or a mark to move it (it snaps to the playhead, the marks and other edges, else to the video's
  frames; hold Shift to snap to frames only), and the video shows the frame at the edge. The mouse wheel zooms around the
  pointer (Shift + wheel or a horizontal wheel scrolls), while zoomed a strip below shows and moves the visible part, and
  the view pages along with the playhead. The zoom is kept for the next start (`ui-state.toml`, `[timeline]`).
- Sections card in the Export panel: the current section's start and end (from the playhead with "Set", the keys I/Home
  and O/End, or typed), its length, and "Add section". Setting a start after the end (or an end before the start) clears
  the other mark, so a new section begins. Added sections are listed with start, end and duration, the number of
  sections and the total length; move the selected one up or down or delete it. A click on a time jumps there, a double
  click edits it (Enter keeps, Escape cancels; reversed times are swapped, times past the end are limited to it). A
  section that is already in the list is not added twice, and overlapping sections are pointed out.
- Sections menu: Set Start, Set End, Add Section, Go to Start, Go to End, Previous and Next Cut Point. View menu: Zoom
  Timeline In, Zoom Timeline Out, Fit Timeline.
- Keys (PyCutRv's plus NLE style): I or Home sets the start, O or End the end, + or Scroll Lock adds the section, Delete
  deletes the selected one, Shift+I / Shift+O go to the start / end, Up / Down jump to the previous / next cut point,
  Left / Right seek 2 s and Shift+Left / Shift+Right 10 s (`editor.seek_step_secs`, `editor.seek_step_long_secs`),
  Page Up / Page Down step a frame like `,` / `.`, J / K / L shuttle (L plays forward and each press doubles the speed
  up to 8x, J plays backward the same way, K stops; the speed shows next to the time), Ctrl++ / Ctrl+- / Ctrl+0 zoom the
  timeline. Ctrl+S and F9 are reserved for saving the job, which arrives with the queue. Held keys repeat only steps,
  seeks and zoom.
- Configurable keymap: `config.toml` takes a `[keymap]` table (`set_start = ["I", "Home"]`, Slint key names such as
  `Control+S`, `Shift+LeftArrow`, `PageUp`, `Plus`, `ScrollLock`); an action that is missing keeps its default keys and
  an empty list unbinds it. Unknown actions and keys are ignored and a key bound to two actions stays with the one that
  was configured (all logged at startup). The menus show the configured keys. The editor for it comes with the Settings
  window. No migration: older `config.toml` files load unchanged.
- Media Info panel: when a video opens, it shows the container (format, duration, start time, size, overall bit rate,
  title) and every stream: codec and profile, resolution, rotation (and the size it is shown at), aspect ratio, frame rate
  (a variable frame rate is detected and shown with its range), frame count, scan type (progressive or interlaced),
  pixel format and bit depth, color, sample rate, channels, sample format, bit rate, language, title, and stream
  durations or start times that differ from the container's (for example audio that ends early). A file that cannot be
  read says why. The information comes from `ffprobe` (setting `advanced.ffprobe`, otherwise from `PATH`); a stalled
  ffprobe is stopped after 20 seconds.
- The MD5 of the open video is shown in the Media Info panel. A sidecar file `.<name>.md5` next to the video is used
  when it exists (it is never changed or deleted); otherwise the MD5 is computed in the background with a progress bar
  and a Cancel button, without slowing down the window. It is computed automatically while the known-source warning
  (`warnings.known_source`) is on, else on demand with "Compute". The warning itself (a banner above the video when
  RustView knows the video already) arrives with the RustView connection.
- Transport bar under the timeline: play/pause, previous and next frame, the current and the total time (`H:MM:SS.mmm`),
  mute and volume. The mouse wheel over the video changes the
  volume (`player.wheel_volume_step`, 5 % per notch). Volume and mute are kept for the next start.
- Playback menu with keys: Space plays or pauses (at the end of a video it starts over), `,` and `.` step one frame back
  and forward, M mutes, F5 reloads the video.
- Opening videos: File > Open (Ctrl+O) shows the desktop's file dialog, starting in the folder of the last video; drag a
  video onto the window (a hint shows while dragging; of several files the first one is opened); `rustcut VIDEO` while
  RustCut runs hands the video to the open window instead of starting a second one (setting `general.single_instance`;
  profiles started with different `--home` folders stay apart).
- A video that cannot be played says why over the video, with a Reload button (click or F5); the rest of the window stays
  usable.
- The window title shows the video and its folder: `<file> (<folder>) - RustCut`. The status line shows messages (for
  example when a dropped item is not a file) and how the video is decoded (hardware or software).
- The player settings are applied: auto play, hardware decoding (`player.hwdec`), the background color around the video,
  muting the last second of a video, and loudness normalization of the preview audio.
- Log files: one file per day in `~/.local/state/rustcut/logs/`; the newest `advanced.log_keep_days` (14) are kept.
- File > Quit (Ctrl+Q).

- Dock layout in RustView's style: the left sidebar holds Export (with collapsible cards) and Render Settings above the
  Queue, the right sidebar holds Tag & Rate, Media Info and Logs. Drag a tab into another group or sidebar, onto the top
  or bottom of a group to split off a new group, or to the window edge to bring back an emptied sidebar; the drop area
  lights up while dragging. Click the active tab (or the chevron) to collapse a group, double-click a tab bar (or use the
  maximize button) to let a group fill its sidebar, collapse a whole sidebar to a rail of panel icons, and resize the
  sidebars and the groups with the bars between them. The layout is kept in `ui-state.toml`; View > Reset Layout
  restores the default arrangement. The panels themselves arrive with their milestones.
- Video playback: a file given on the command line (`rustcut VIDEO`) plays in the main window. mpv draws into RustCut's own
  OpenGL context, so later overlays (crop handles, section markers) can be drawn directly over the video. On a software
  OpenGL renderer (for example llvmpipe in a virtual machine) the preview uses mpv's fast scaling automatically, so playback
  keeps the video's frame rate; the rendered output is not affected.
- First runnable build: RustCut opens its main window in RustView's color palette, with a one-line status bar.
- Command line: `rustcut [FILE]... [--home DIR]` (`--home` keeps settings, data and logs in an isolated folder).
- The window size, position and maximized state are restored at the next start (`~/.config/rustcut/ui-state.toml`).
  Closing the window, File > Quit and the signals SIGTERM, SIGINT and SIGHUP all save the window, the layout and the
  volume before RustCut ends; a second signal ends it at once. A stored position far off every screen is dropped and
  `advanced.log_keep_days = 0` is read as 1 (no migration: the files keep their version).
- On X11 the window class is `rustcut` / `io.github.vommie.rustcut`, so the task manager groups the window correctly.
- For development: debug builds include Slint's MCP server; started with `SLINT_MCP_PORT=<port>`, a running RustCut can be
  inspected and driven from an MCP client (element tree, screenshots, clicks). Without the variable nothing listens.
- Settings (`~/.config/rustcut/config.toml`) and the UI state are versioned: files of older versions are backed up to
  `~/.config/rustcut/backups/` before they are migrated, files of newer versions are never overwritten, and damaged files
  are kept as a backup while defaults are used. The log level can be set with `advanced.log_level` (`RUST_LOG` wins).

### Changed

- The close question while a job renders follows the queue: while it is open no new job starts (the running ones go
  on). Once no job renders anymore (the running render or the whole queue is done), it says so and RustCut quits by
  itself after 3 s; "Stay" (Escape) keeps RustCut open and lets the queue go on. With "when done" set to quit,
  suspend or power off, that action runs instead (and RustCut quits after a suspend).
- Settings changed in the Settings window apply while RustCut runs where they can: hold time, tooltips, keys, seek
  steps, auto play, wheel step, preview audio filters, the export card's folders, template and warnings, the
  automatic section, filter defaults, the draft rule when a job is edited, render profiles, ffmpeg's priority and
  stall limit, the number of jobs at the same time, the RustView link and the tag history size, the log level and
  how long log files are kept, hardware decoding and the background color of the player (also while a video plays),
  the ffmpeg and ffprobe programs (the encoders are checked again), the known-source warning, and "One window" (a
  second start opens its own window as soon as it is off). Only the tray icon still needs a new start.
- Closing the main window quits RustCut even while the Settings window is open.
- The Render Settings panel's tab is called "Codecs" (shorter; your layout keeps its place).
- The queue panel's actions apply to every selected job; its buttons sit below the list.
- The Sections card's buttons are icons with tooltips (set start/end to the playhead, add, move, delete).

### Removed

- "Divisible by" in the Crop menu (0.0.60): the rule now applies to the finished video (Settings > Filters > Final
  size); a dragged or found crop only stays even. The kept crop multiple in `ui-state.toml` is no longer read.
- The size hint on the Filters card's header and its message kind "Output size" (0.0.60): the research showed that
  sizes that are no multiple of 8 or 16 cause no problem with any encoder here, and an odd size is handled by the
  final size rule; the header's tooltip says what the rule did.
- Updating a queued job from the editor, and the message kind "Edited job updated" (0.0.35): loading a job is always
  a copy now.
- "Duplicate" of a queued job (context menu and Job panel), on the user's decision (0.0.31): load the job into the
  editor and save it under another name instead.

### Fixed

- Picking a border color with the pipette on a video's last frame failed with "The color could not be read"
  (0.0.56): the color is now read from the frame shown there.
- The status line no longer keeps "Not saved: …" of an earlier attempt after the job was saved (0.0.33).
- "Render Again" of a finished job failed at once ("exists already and overwriting was not chosen") (0.0.31): the job
  renders again and replaces its own output once the new render is verified (a failed or canceled job still never
  overwrites a file).
- The keys of the menu bar (I, O, Shift+I, the frame steps, ...) acted on the editor behind an open dialog, e.g. the
  question about missing tags (0.0.31). Since 0.0.32 the menu entries are also off while a dialog is open (Quit
  stays).
- The time under the video no longer shows a moment past the video's length at its end (e.g. 0:00:10.007 /
  0:00:10.000 on an MPEG program stream) (0.0.32).
- A file that does not play no longer shows the target name bubble over the dimmed Job Finalization card, nor
  "checking…" next to Deinterlace (0.0.31).
- A render no longer goes on after RustCut crashed (0.0.30): when RustCut was killed or crashed while a job rendered,
  ffmpeg went on encoding in the background, and after the next start the same job rendered a second time beside it.
  At start, RustCut now stops the renders of its own interrupted jobs that an earlier RustCut left behind (and those
  of an interrupted probe render) before it cleans up their work files.
- Audio "Copy (no re-encode)" works (0.0.23): a job with one section takes the source's audio tracks unchanged.
  Before, every job with audio failed when it rendered. With several sections, or when the container cannot take the
  source's audio (e.g. AAC in WebM), Save is off and its tooltip names the reason before the job is saved. Since
  0.0.24 the copied audio starts exactly with the section in MKV and WebM too (it started up to a second early and
  out of sync with the picture).
- QuickTime (.mov) with Opus or FLAC audio (0.0.23) or with AV1 video (0.0.24) is marked as an error in the Codecs
  panel (ffmpeg refuses it; it was only a warning, and the render failed).
- A `rustview` launcher that cannot find its program (exit code 126 or 127, e.g. a wrapper script) shows RustView as
  unavailable with the reason, instead of as connected without tags (0.0.23).
- The Tag & Rate panel lays out the tag tree like RustView again (0.0.22): the tags run down a column and a new
  column starts only when the area is full. Before, every tag got a column of its own, so the tags stood side by side
  in one row.
- The last progress of a render (often its end) is no longer lost when ffmpeg writes it just before it exits.
- Starting ffmpeg, ffprobe or rustview tries again for a moment while the system reports the program as busy (for
  example while a package update replaces it) instead of failing the job at once.
- Quitting RustCut while a video without audio is open no longer logs "The file cannot be played: The video output
  could not be started." (0.0.14). Closing the window takes the video output away from the player, which is a normal
  end of playback, not a failure; a real failure of the video output still shows the error over the video.
- FLAC audio honors the channel count and the sample rate of a profile (both were ignored).

[Unreleased]: https://github.com/vommie/rustcut/compare/v0.0.76...HEAD
[0.0.76]: https://github.com/vommie/rustcut/compare/v0.0.75...v0.0.76
[0.0.75]: https://github.com/vommie/rustcut/compare/v0.0.71...v0.0.75
[0.0.71]: https://github.com/vommie/rustcut/compare/v0.0.69...v0.0.71
[0.0.69]: https://github.com/vommie/rustcut/compare/v0.0.68...v0.0.69
[0.0.68]: https://github.com/vommie/rustcut/compare/v0.0.66...v0.0.68
[0.0.66]: https://github.com/vommie/rustcut/compare/v0.0.60...v0.0.66
[0.0.60]: https://github.com/vommie/rustcut/releases/tag/v0.0.60
