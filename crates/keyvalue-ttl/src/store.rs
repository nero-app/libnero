use std::{collections::HashMap, fmt::Write, path::PathBuf, sync::Arc};

use anyhow::Result;
use sha2::{Digest, Sha256};
use tokio::{fs, sync::Mutex, task::JoinSet};

use crate::{Error, file::FileEntry};

type FileLock = Arc<Mutex<()>>;

pub struct FileStore {
    root: PathBuf,
    file_locks: Mutex<HashMap<PathBuf, FileLock>>,
    size_bytes: Mutex<u64>,
    max_bytes: Option<u64>,
}

impl FileStore {
    pub async fn new(root: PathBuf, max_bytes: Option<u64>) -> Result<Self> {
        fs::create_dir_all(&root).await?;
        let initial = Self::scan_bytes(&root).await;

        Ok(Self {
            root,
            file_locks: Mutex::new(HashMap::new()),
            size_bytes: Mutex::new(initial),
            max_bytes,
        })
    }

    async fn scan_bytes(dir: &PathBuf) -> u64 {
        let mut total = 0u64;
        let mut set = JoinSet::new();

        set.spawn(Self::scan_dir(dir.clone()));

        while let Some(res) = set.join_next().await {
            if let Ok((size, subdirs)) = res {
                total += size;
                for subdir in subdirs {
                    set.spawn(Self::scan_dir(subdir));
                }
            }
        }

        total
    }

    async fn scan_dir(dir: PathBuf) -> (u64, Vec<PathBuf>) {
        let mut size = 0u64;
        let mut subdirs = vec![];

        let mut rd = match fs::read_dir(&dir).await {
            Ok(r) => r,
            Err(_) => return (0, vec![]),
        };

        while let Ok(Some(entry)) = rd.next_entry().await {
            if entry.file_name().to_string_lossy().ends_with("~tmp") {
                continue;
            }

            match entry.metadata().await {
                Ok(m) if m.is_dir() => subdirs.push(entry.path()),
                Ok(m) => size += m.len(),
                Err(_) => {}
            }
        }

        (size, subdirs)
    }

    fn key_to_path(&self, key: &str) -> PathBuf {
        let hash = Sha256::digest(key.as_bytes());
        let mut hex = String::with_capacity(64);
        for b in &hash {
            write!(hex, "{:02x}", b).unwrap();
        }
        let (prefix, tail) = hex.split_at(2);
        self.root.join(prefix).join(tail)
    }

    async fn lock_for(&self, path: &PathBuf) -> FileLock {
        let mut map = self.file_locks.lock().await;
        map.retain(|_, v| Arc::strong_count(v) > 1);
        map.entry(path.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        let path = self.key_to_path(key);
        let file_lock = self.lock_for(&path).await;
        let _guard = file_lock.lock().await;

        let data = match fs::read(&path).await {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Error::from(e)),
        };

        let entry = FileEntry::try_from(data.as_slice())?;

        if entry.is_expired() {
            let _ = fs::remove_file(&path).await;
            let mut sz = self.size_bytes.lock().await;
            *sz = sz.saturating_sub(data.len() as u64);
            return Ok(None);
        }

        Ok(Some(entry.value))
    }

    pub async fn set(&self, key: &str, value: Vec<u8>, ttl_ms: Option<u32>) -> Result<(), Error> {
        let path = self.key_to_path(key);
        let file_lock = self.lock_for(&path).await;
        let _guard = file_lock.lock().await;

        let old_bytes = fs::metadata(&path).await.map(|m| m.len()).unwrap_or(0);

        let entry = match ttl_ms {
            Some(ttl) => FileEntry::new(key.to_owned(), value).with_ttl(ttl),
            None => FileEntry::new(key.to_owned(), value),
        };
        let buf = entry.encode();
        let new_bytes = buf.len() as u64;

        let mut sz = self.size_bytes.lock().await;
        let projected = sz.saturating_sub(old_bytes) + new_bytes;
        if self.max_bytes.is_some_and(|max| projected > max) {
            return Err(Error::StorageLimitExceeded);
        }

        let parent = path.parent().unwrap();
        fs::create_dir_all(parent).await?;
        let tmp = path.with_extension("~tmp");
        fs::write(&tmp, &buf).await?;
        fs::rename(&tmp, &path).await?;

        *sz = projected;
        Ok(())
    }

    pub async fn delete(&self, key: &str) -> Result<(), Error> {
        let path = self.key_to_path(key);
        let file_lock = self.lock_for(&path).await;
        let _guard = file_lock.lock().await;

        let old_bytes = fs::metadata(&path).await.map(|m| m.len()).unwrap_or(0);
        if old_bytes == 0 {
            return Ok(());
        }

        match fs::remove_file(&path).await {
            Ok(()) => {
                let mut sz = self.size_bytes.lock().await;
                *sz = sz.saturating_sub(old_bytes);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::from(e)),
        }
    }

    pub async fn exists(&self, key: &str) -> Result<bool, Error> {
        Ok(self.get(key).await?.is_some())
    }

    pub async fn list_keys(
        &self,
        cursor: Option<&str>,
    ) -> Result<(Vec<String>, Option<String>), Error> {
        const PAGE_SIZE: usize = 100;

        let mut keys = Vec::new();
        let mut stack = vec![self.root.clone()];

        while let Some(d) = stack.pop() {
            let mut rd = match fs::read_dir(&d).await {
                Ok(r) => r,
                Err(_) => continue,
            };

            while let Ok(Some(entry)) = rd.next_entry().await {
                let Ok(ft) = entry.file_type().await else {
                    continue;
                };

                if ft.is_dir() {
                    stack.push(entry.path());
                    continue;
                }

                if entry.file_name().to_string_lossy().ends_with("~tmp") {
                    continue;
                }

                let Ok(data) = fs::read(entry.path()).await else {
                    continue;
                };
                let Ok(fe) = FileEntry::try_from(data.as_slice()) else {
                    continue;
                };

                if !fe.is_expired() {
                    keys.push(fe.key);
                }
            }
        }

        keys.sort_unstable();

        let start = cursor.map_or(0, |c| keys.partition_point(|k| k.as_str() <= c));
        let total = keys.len();
        let page = keys[start..]
            .iter()
            .take(PAGE_SIZE)
            .cloned()
            .collect::<Vec<_>>();
        let cursor = (start + PAGE_SIZE < total)
            .then(|| page.last().cloned())
            .flatten();

        Ok((page, cursor))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::{Error, store::FileStore};

    async fn store(max_bytes: Option<u64>) -> (FileStore, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let store = FileStore::new(dir.path().to_path_buf(), max_bytes)
            .await
            .unwrap();
        (store, dir)
    }

    #[tokio::test]
    async fn set_and_get() {
        let (store, _dir) = store(None).await;
        store.set("hello", b"world".to_vec(), None).await.unwrap();
        assert_eq!(store.get("hello").await.unwrap(), Some(b"world".to_vec()));
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let (store, _dir) = store(None).await;
        assert_eq!(store.get("missing").await.unwrap(), None);
    }

    #[tokio::test]
    async fn delete_removes_key() {
        let (store, _dir) = store(None).await;
        store.set("k", b"v".to_vec(), None).await.unwrap();
        store.delete("k").await.unwrap();
        assert_eq!(store.get("k").await.unwrap(), None);
    }

    #[tokio::test]
    async fn delete_missing_key_is_noop() {
        let (store, _dir) = store(None).await;
        assert!(store.delete("missing").await.is_ok());
    }

    #[tokio::test]
    async fn ttl_expiry() {
        let (store, _dir) = store(None).await;
        store.set("k", b"v".to_vec(), Some(1)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        assert_eq!(store.get("k").await.unwrap(), None);
    }

    #[tokio::test]
    async fn no_ttl_does_not_expire() {
        let (store, _dir) = store(None).await;
        store.set("k", b"v".to_vec(), None).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        assert_eq!(store.get("k").await.unwrap(), Some(b"v".to_vec()));
    }

    #[tokio::test]
    async fn max_bytes_enforced() {
        let (store, _dir) = store(Some(20)).await;
        store.set("k1", b"v".to_vec(), None).await.unwrap();
        let err = store.set("k2", b"v".to_vec(), None).await.unwrap_err();
        assert!(matches!(err, Error::StorageLimitExceeded));
    }

    #[tokio::test]
    async fn overwrite_same_key_does_not_accumulate_size() {
        let (store, _dir) = store(Some(1024)).await;
        store.set("k", b"aaa".to_vec(), None).await.unwrap();
        store.set("k", b"bbb".to_vec(), None).await.unwrap();
        let sz = *store.size_bytes.lock().await;
        assert!(sz < 1024);
    }

    #[tokio::test]
    async fn list_keys_returns_original_strings() {
        let (store, _dir) = store(None).await;
        store.set("alpha", b"1".to_vec(), None).await.unwrap();
        store.set("beta", b"2".to_vec(), None).await.unwrap();
        store.set("gamma", b"3".to_vec(), None).await.unwrap();
        let (keys, cursor) = store.list_keys(None).await.unwrap();
        assert_eq!(keys, vec!["alpha", "beta", "gamma"]);
        assert!(cursor.is_none());
    }

    #[tokio::test]
    async fn list_keys_excludes_expired() {
        let (store, _dir) = store(None).await;
        store.set("live", b"1".to_vec(), None).await.unwrap();
        store.set("dead", b"2".to_vec(), Some(1)).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let (keys, _) = store.list_keys(None).await.unwrap();
        assert_eq!(keys, vec!["live"]);
    }

    #[tokio::test]
    async fn concurrent_writes_respect_size_limit() {
        use std::sync::Arc;
        let dir = tempdir().unwrap();
        let store = Arc::new(
            FileStore::new(dir.path().to_path_buf(), Some(100))
                .await
                .unwrap(),
        );

        let mut set = tokio::task::JoinSet::new();
        for i in 0..10 {
            let store = Arc::clone(&store);
            set.spawn(async move { store.set(&format!("key{}", i), vec![0u8; 20], None).await });
        }

        let mut limit_errors = 0;
        while let Some(res) = set.join_next().await {
            if matches!(res.unwrap(), Err(Error::StorageLimitExceeded)) {
                limit_errors += 1;
            }
        }

        let sz = *store.size_bytes.lock().await;
        assert!(sz <= 100, "size {} exceeded the limit", sz);
        assert!(
            limit_errors > 0,
            "no writes were rejected, limit was not enforced"
        );
    }
}
