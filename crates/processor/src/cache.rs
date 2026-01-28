use std::{
    collections::HashMap,
    hash::Hash,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::RwLock, time};

#[derive(Debug, Clone)]
struct CacheEntry<V> {
    value: V,
    expires_at: Option<Instant>,
}

impl<V> CacheEntry<V> {
    fn new(value: V, ttl: Option<Duration>) -> Self {
        Self {
            value,
            expires_at: ttl.map(|duration| Instant::now() + duration),
        }
    }

    fn is_expired(&self) -> bool {
        self.expires_at
            .map(|expires_at| Instant::now() >= expires_at)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
pub struct Cache<K, V> {
    map: Arc<RwLock<HashMap<K, CacheEntry<V>>>>,
    ttl: Option<Duration>,
    max_capacity: Option<usize>,
}

impl<K, V> Default for Cache<K, V> {
    fn default() -> Self {
        Self {
            map: Arc::new(RwLock::new(HashMap::new())),
            ttl: None,
            max_capacity: None,
        }
    }
}

impl<K, V> Cache<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self.spawn_cleanup_task();
        self
    }

    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.max_capacity = Some(capacity);
        self
    }

    fn spawn_cleanup_task(&self) {
        if self.ttl.is_none() {
            return;
        }

        let map = self.map.clone();

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(60));

            loop {
                interval.tick().await;
                let mut entries = map.write().await;
                entries.retain(|_, entry| !entry.is_expired());
            }
        });
    }

    pub async fn insert(&self, key: K, value: V) -> bool {
        let mut map = self.map.write().await;

        if let Some(max) = self.max_capacity
            && map.len() >= max
            && !map.contains_key(&key)
        {
            return false;
        }

        map.insert(key, CacheEntry::new(value, self.ttl));
        true
    }

    pub async fn get(&self, key: &K) -> Option<V> {
        let map = self.map.read().await;
        let entry = map.get(key)?;

        if entry.is_expired() {
            return None;
        }

        Some(entry.value.clone())
    }

    pub async fn remove(&self, key: &K) -> Option<V> {
        let mut map = self.map.write().await;
        let entry = map.remove(key)?;

        if entry.is_expired() {
            return None;
        }

        Some(entry.value)
    }

    #[allow(unused)]
    pub async fn clear(&self) {
        let mut map = self.map.write().await;
        map.clear();
    }

    #[allow(unused)]
    pub async fn len(&self) -> usize {
        let map = self.map.read().await;
        map.len()
    }

    #[allow(unused)]
    pub async fn is_empty(&self) -> bool {
        let map = self.map.read().await;
        map.is_empty()
    }
}
