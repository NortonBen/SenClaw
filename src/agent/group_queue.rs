//! Per-group message serialization + global concurrency control.
//! Mirrors `src-old/agent/GroupQueue.ts`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::{Mutex, Semaphore};

type BoxedTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

struct Inner {
    queues: Mutex<HashMap<String, Vec<BoxedTask>>>,
    running: Mutex<HashMap<String, bool>>,
}

pub struct GroupQueue {
    inner: Arc<Inner>,
    semaphore: Arc<Semaphore>,
}

impl GroupQueue {
    pub fn new(max_concurrent: u32) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(Inner {
                queues: Mutex::new(HashMap::new()),
                running: Mutex::new(HashMap::new()),
            }),
            semaphore: Arc::new(Semaphore::new(max_concurrent as usize)),
        })
    }

    pub async fn enqueue(self: &Arc<Self>, jid: &str, task: BoxedTask) {
        self.inner
            .queues
            .lock()
            .await
            .entry(jid.to_string())
            .or_default()
            .push(task);

        let was_idle = {
            let mut running = self.inner.running.lock().await;
            if running.get(jid).copied().unwrap_or(false) {
                false
            } else {
                running.insert(jid.to_string(), true);
                true
            }
        };

        if was_idle {
            let this = Arc::clone(self);
            let jid = jid.to_string();
            tokio::spawn(run_drain(this, jid));
        }
    }

    pub async fn clear_queue(&self, jid: &str) {
        let mut queues = self.inner.queues.lock().await;
        if let Some(q) = queues.get_mut(jid) {
            if !q.is_empty() {
                tracing::info!(
                    "[GroupQueue] Clearing {} pending task(s) for {jid}",
                    q.len()
                );
                q.clear();
            }
        }
    }
}

/// Standalone drain loop — owns the Arc, so futures are 'static + Send.
fn run_drain(
    gq: Arc<GroupQueue>,
    jid: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move {
        let permit = gq.semaphore.acquire().await.expect("semaphore closed");

        loop {
            let next = {
                let mut queues = gq.inner.queues.lock().await;
                queues.get_mut(&jid).and_then(|q| {
                    if q.is_empty() {
                        None
                    } else {
                        Some(q.remove(0))
                    }
                })
            };

            match next {
                Some(task) => {
                    task.await;
                }
                None => break,
            }
        }

        // Explicitly drop permit before mutating gq for re-scheduling
        drop(permit);
        gq.inner.running.lock().await.remove(&jid);

        let has_more = gq
            .inner
            .queues
            .lock()
            .await
            .get(&jid)
            .map(|q| !q.is_empty())
            .unwrap_or(false);

        if has_more {
            gq.inner.running.lock().await.insert(jid.clone(), true);
            tokio::spawn(run_drain(gq, jid));
        }
    })
}
