//! The jobs' own pictures (P12 block 10): one small picture per job for the queue's job card, stored with the job and
//! deleted with it (`ON DELETE CASCADE`). The queue makes it when the job is saved (`queue::picture`).

use rusqlite::{OptionalExtension as _, params};

use super::{Store, StoreError};
use crate::job::JobId;

/// The only format written so far; a row of another format is skipped (a newer RustCut may write others).
pub const WEBP: &str = "webp";

/// A job's picture as stored: the encoded bytes and the picture's size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobPicture {
    pub width: u32,
    pub height: u32,
    /// WebP bytes.
    pub webp: Vec<u8>,
}

impl Store {
    /// Stores (or replaces) the picture of job `id`.
    pub fn set_picture(&mut self, id: JobId, picture: &JobPicture) -> Result<(), StoreError> {
        let exists: Option<i64> = self
            .conn
            .query_row("SELECT id FROM jobs WHERE id = ?1", [id.0], |r| r.get(0))
            .optional()?;
        if exists.is_none() {
            return Err(StoreError::NoSuchJob(id.0));
        }
        self.conn.execute(
            "INSERT OR REPLACE INTO job_pictures (job_id, format, width, height, picture) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id.0, WEBP, picture.width, picture.height, picture.webp],
        )?;
        Ok(())
    }

    /// The picture of job `id`, if it has one this version can read.
    pub fn picture(&self, id: JobId) -> Result<Option<JobPicture>, StoreError> {
        let row = self
            .conn
            .query_row(
                "SELECT format, width, height, picture FROM job_pictures WHERE job_id = ?1",
                [id.0],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, u32>(1)?,
                        r.get::<_, u32>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                    ))
                },
            )
            .optional()?;
        Ok(row
            .filter(|(format, ..)| format == WEBP)
            .map(|(_, width, height, webp)| JobPicture { width, height, webp }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{JobState, sample_spec};

    #[test]
    fn a_picture_goes_with_its_job() {
        let mut store = Store::open_in_memory().unwrap();
        let job = store.insert_job(&sample_spec(), &JobState::Waiting).unwrap();
        assert_eq!(store.picture(job.id).unwrap(), None);
        let picture = JobPicture {
            width: 160,
            height: 90,
            webp: vec![1, 2, 3],
        };
        store.set_picture(job.id, &picture).unwrap();
        assert_eq!(store.picture(job.id).unwrap(), Some(picture.clone()));
        let other = JobPicture {
            webp: vec![4],
            ..picture
        };
        store.set_picture(job.id, &other).unwrap();
        assert_eq!(store.picture(job.id).unwrap(), Some(other), "replaced");
        store.delete_jobs(&[job.id]).unwrap();
        let left: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM job_pictures", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0, "deleted with its job");
        assert!(matches!(
            store.set_picture(
                job.id,
                &JobPicture {
                    width: 1,
                    height: 1,
                    webp: vec![]
                }
            ),
            Err(StoreError::NoSuchJob(_))
        ));
    }

    #[test]
    fn a_picture_of_an_unknown_format_is_skipped() {
        let mut store = Store::open_in_memory().unwrap();
        let job = store.insert_job(&sample_spec(), &JobState::Waiting).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO job_pictures (job_id, format, width, height, picture) VALUES (?1, 'avif', 1, 1, x'00')",
                [job.id.0],
            )
            .unwrap();
        assert_eq!(store.picture(job.id).unwrap(), None);
    }
}
