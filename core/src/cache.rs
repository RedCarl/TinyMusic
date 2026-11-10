//! 进程内 TTL 缓存。用于搜索结果、详情、取链结果，压低对限流接口的请求量。

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct TtlCache<K, V> {
    map: Mutex<HashMap<K, (Instant, V)>>,
    ttl: Duration,
    cap: usize,
}

impl<K: Eq + Hash + Clone, V: Clone> TtlCache<K, V> {
    pub fn new(ttl: Duration, cap: usize) -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            ttl,
            cap,
        }
    }

    pub fn get(&self, k: &K) -> Option<V> {
        let mut map = self.map.lock().ok()?;
        match map.get(k) {
            Some((t, v)) if t.elapsed() < self.ttl => Some(v.clone()),
            Some(_) => {
                map.remove(k);
                None
            }
            None => None,
        }
    }

    pub fn put(&self, k: K, v: V) {
        let Ok(mut map) = self.map.lock() else { return };
        if map.len() >= self.cap {
            // 简单淘汰：清掉过期项，仍满则整体清空（这些缓存都可重建）
            map.retain(|_, (t, _)| t.elapsed() < self.ttl);
            if map.len() >= self.cap {
                map.clear();
            }
        }
        map.insert(k, (Instant::now(), v));
    }

    pub fn clear(&self) {
        if let Ok(mut map) = self.map.lock() {
            map.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.map.lock().map(|m| m.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_and_expires() {
        let c: TtlCache<String, u32> = TtlCache::new(Duration::from_millis(40), 8);
        c.put("a".into(), 1);
        assert_eq!(c.get(&"a".into()), Some(1));
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(c.get(&"a".into()), None);
    }
}
