use alloc::vec::Vec;
use core::{
  pin::Pin,
  task::{Context, Poll},
};

use futures_core::{Stream, ready};
use pin_project_lite::pin_project;

pin_project! {
  pub struct StreamMap<K, V> {
    entries: Vec<(K, V)>,
  }
}

impl<K, V> StreamMap<K, V> {
  pub fn new() -> Self {
    StreamMap {
      entries: Vec::new(),
    }
  }

  pub fn with_capacity(capacity: usize) -> Self {
    StreamMap {
      entries: Vec::with_capacity(capacity),
    }
  }

  pub fn insert(&mut self, key: K, value: V) {
    self.entries.push((key, value))
  }

  pub fn remove<Q>(&mut self, key: &Q) -> Option<(K, V)>
  where
    Q: PartialEq<K>,
  {
    self
      .entries
      .iter()
      .position(|(k, _)| key == k)
      .map(|index| self.entries.swap_remove(index))
  }
}

impl<K, V> StreamMap<K, V>
where
  V: Stream + Unpin,
{
  fn poll_next_entry(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<(usize, V::Item)>> {
    let this = self.project();

    if this.entries.is_empty() {
      return Poll::Ready(None);
    }

    let start = fastrand::usize(..this.entries.len());
    let mut idx = start;

    for _ in 0..this.entries.len() {
      let (_, stream) = &mut this.entries[idx];

      match Pin::new(stream).poll_next(cx) {
        Poll::Ready(Some(val)) => return Poll::Ready(Some((idx, val))),
        Poll::Ready(None) => {
          // Remove the entry
          this.entries.swap_remove(idx);

          // Check if this was the last entry, if so the cursor needs
          // to wrap
          if idx == this.entries.len() {
            idx = 0;
          } else if idx < start && start <= this.entries.len() {
            // The stream being swapped into the current index has
            // already been polled, so skip it.
            idx = idx.wrapping_add(1) % this.entries.len();
          }
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

impl<K, V> Default for StreamMap<K, V> {
  fn default() -> Self {
    Self::new()
  }
}

impl<K, V> Stream for StreamMap<K, V>
where
  K: Clone,
  V: Stream + Unpin,
{
  type Item = (K, V::Item);

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    match ready!(self.as_mut().poll_next_entry(cx)) {
      Some((index, value)) => {
        let key = self.entries[index].0.clone();
        Poll::Ready(Some((key, value)))
      }
      None => Poll::Ready(None),
    }
  }
}

#[cfg(test)]
mod tests {

  use alloc::{boxed::Box, collections::BTreeMap};

  use smol::stream::StreamExt;

  use super::*;

  #[test]
  fn basic_stream_map() {
    let mut streams = StreamMap::default();

    {
      let (tx, rx) = async_channel::bounded(1);

      streams.insert(1, Box::pin(rx));
      smol::spawn(async move {
        let _ = tx.send("foobar").await;
      })
      .detach();
    }

    {
      let (tx, rx) = async_channel::bounded(1);

      streams.insert(2, Box::pin(rx));
      smol::spawn(async move {
        let _ = tx.send("foobaz").await;
      })
      .detach();
    }

    assert_eq!(
      smol::block_on(streams.collect::<BTreeMap<_, _>>()),
      BTreeMap::from([(1, "foobar"), (2, "foobaz")])
    );
  }
}
