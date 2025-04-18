use core::{
  pin::Pin,
  task::{Context, Poll},
};

use futures_core::Stream;
use pin_project_lite::pin_project;

pin_project! {
  pub struct FuturesMap<K, V> {
    entries: Vec<(K, V)>,
  }
}

impl<K, V> FuturesMap<K, V> {
  pub fn new() -> Self {
    FuturesMap {
      entries: Vec::new(),
    }
  }

  pub fn with_capacity(capacity: usize) -> Self {
    FuturesMap {
      entries: Vec::with_capacity(capacity),
    }
  }

  pub fn insert(&mut self, key: K, value: V) {
    self.entries.push((key, value))
  }
}

impl<K, V> Default for FuturesMap<K, V> {
  fn default() -> Self {
    Self::new()
  }
}

impl<K, V> Stream for FuturesMap<K, V>
where
  V: Future + Unpin,
{
  type Item = (K, V::Output);

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let this = self.project();

    if this.entries.is_empty() {
      return Poll::Ready(None);
    }

    let start = fastrand::usize(..this.entries.len());
    let mut idx = start;

    for _ in 0..this.entries.len() {
      let (_, stream) = &mut this.entries[idx];

      match Pin::new(stream).poll(cx) {
        Poll::Ready(val) => {
          let (key, _) = this.entries.swap_remove(idx);
          return Poll::Ready(Some((key, val)));
        }
        Poll::Pending => {
          idx = idx.wrapping_add(1) % this.entries.len();
        }
      }
    }

    // If the map is empty, then the stream is complete.
    if this.entries.is_empty() {
      Poll::Ready(None)
    } else {
      Poll::Pending
    }
  }
}
