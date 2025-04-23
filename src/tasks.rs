use alloc::{boxed::Box, collections::VecDeque};
use core::{
  any::TypeId,
  ops::{ControlFlow, Deref, DerefMut},
  pin::Pin,
  task::{Context, Poll},
};
use fastrand::Rng;

use async_channel::{Receiver, Sender};
use futures_core::{Stream, ready};
use pin_project_lite::pin_project;

use crate::{BackgroundTask, MessageBus, future::FuturesMap, stream::StreamMap};

pin_project! {
  /// Map to register [`BackgroundTask`] and [`Stream`] the [`TaskUpdate`]s form said tasks.
  ///
  /// There are 2 primary metods of using BackgroundTasks is with [`BackgroundTasks::register`] or
  /// [`BackgroundTasks::register_typed`] where the register_typed variant uses [`TypeId`] of backround task as key.
  ///
  /// Note: it will also try and drain any remaining messages on channel to yield them before `TaskUpdate::Finished`.
  pub struct BackgroundTasks<K, MessageOut, Output, Task> {
    #[pin]
    streams: StreamMap<K, Pin<Box<Receiver<MessageOut>>>>,

    #[pin]
    handles: FuturesMap<K, Task>,

    rng: Rng,
    task_drain_queue: VecDeque<(K, TaskUpdate<MessageOut, Output>)>,
  }
}

impl<K, MessageOut, Output, Task> BackgroundTasks<K, MessageOut, Output, Task> {
  pub fn new(mut rng: Rng) -> Self {
    BackgroundTasks {
      streams: StreamMap::new(rng.fork()),
      handles: FuturesMap::new(rng.fork()),
      rng,
      task_drain_queue: VecDeque::new(),
    }
  }

  pub fn with_capacity(mut rng: Rng, capacity: usize) -> Self {
    BackgroundTasks {
      streams: StreamMap::with_capacity(rng.fork(), capacity),
      handles: FuturesMap::with_capacity(rng.fork(), capacity),
      rng,
      task_drain_queue: VecDeque::new(),
    }
  }

  pub fn is_empty(&self) -> bool {
    self.streams.is_empty() && self.handles.is_empty()
  }

  pub fn contains<Q>(&mut self, key: &Q) -> bool
  where
    Q: PartialEq<K>,
  {
    self.streams.contains(key) || self.handles.contains(key)
  }

  /// Register new [`BackgroundTask`] with a specific key, this key is used to sync up the message
  /// bus channels and started task.
  #[must_use]
  pub fn register<T>(&mut self, key: K, task: T) -> TaskSender<T>
  where
    K: Clone + PartialEq,
    T: BackgroundTask<MessageOut = MessageOut, Task = Task>,
  {
    let (in_tx, in_rx) = async_channel::unbounded::<T::MessageIn>();
    let (out_tx, out_rx) = async_channel::unbounded::<T::MessageOut>();

    let message_bus = MessageBus::from_parts(out_tx, in_rx);
    self.streams.insert(key.clone(), Box::pin(out_rx));
    self.handles.insert(key, task.run(message_bus));

    TaskSender(in_tx)
  }
}

impl<MessageOut, Output, Task> BackgroundTasks<TypeId, MessageOut, Output, Task> {
  pub fn contains_typed<T>(&mut self) -> bool
  where
    T: BackgroundTask<MessageOut = MessageOut, Task = Task> + 'static,
  {
    self.contains(&TypeId::of::<T>())
  }

  #[must_use]
  pub fn register_typed<T>(&mut self, task: T) -> TaskSender<T>
  where
    T: BackgroundTask<MessageOut = MessageOut, Task = Task> + 'static,
  {
    self.register(TypeId::of::<T>(), task)
  }
}

impl<K, MessageOut, Output, Task> BackgroundTasks<K, MessageOut, Output, Task>
where
  K: Clone + PartialEq,
  Task: Future<Output = Output> + Unpin,
{
  fn poll_next_drain_queue(
    self: Pin<&mut Self>,
  ) -> ControlFlow<Poll<Option<(K, TaskUpdate<MessageOut, Output>)>>> {
    if let result @ Some(_) = self.project().task_drain_queue.pop_front() {
      ControlFlow::Break(Poll::Ready(result))
    } else {
      ControlFlow::Continue(())
    }
  }

  fn poll_next_message(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
  ) -> Poll<Option<(K, TaskUpdate<MessageOut, Output>)>> {
    let next = ready!(self.project().streams.poll_next(cx));
    Poll::Ready(next.map(|(key, message)| (key, TaskUpdate::Message(message))))
  }

  fn poll_next_handle(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
  ) -> ControlFlow<Poll<Option<(K, TaskUpdate<MessageOut, Output>)>>> {
    let mut this = self.project();

    match this.handles.poll_next(cx) {
      Poll::Pending => ControlFlow::Break(Poll::Pending),
      Poll::Ready(None) => ControlFlow::Break(Poll::Ready(None)),
      Poll::Ready(Some((key, result))) => {
        let mut had_messages = false;

        if let Some((_, streams)) = this.streams.remove(&key) {
          while let Ok(message) = streams.try_recv() {
            this
              .task_drain_queue
              .push_back((key.clone(), TaskUpdate::Message(message)));

            had_messages = true;
          }
        }
        if had_messages {
          this
            .task_drain_queue
            .push_back((key, TaskUpdate::Finished(result)));

          ControlFlow::Continue(())
        } else {
          ControlFlow::Break(Poll::Ready(Some((key, TaskUpdate::Finished(result)))))
        }
      }
    }
  }
}

#[cfg(feature = "std")]
impl<K, MessageOut, Output, Task> Default for BackgroundTasks<K, MessageOut, Output, Task> {
  fn default() -> Self {
    Self::new(Default::default())
  }
}

impl<K, MessageOut, Output, Task> Stream for BackgroundTasks<K, MessageOut, Output, Task>
where
  K: Clone + PartialEq,
  Task: Future<Output = Output> + Unpin,
{
  type Item = (K, TaskUpdate<MessageOut, Output>);

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    loop {
      if let Some(result) = self.as_mut().poll_next_drain_queue().break_value() {
        break result;
      }

      if self.rng.bool() {
        match self.as_mut().poll_next_handle(cx) {
          ControlFlow::Continue(()) => continue,
          ControlFlow::Break(result @ Poll::Ready(Some(_))) => break result,
          ControlFlow::Break(_) => {}
        }

        if let result @ Poll::Ready(Some(_)) = self.as_mut().poll_next_message(cx) {
          break result;
        }
      } else {
        if let result @ Poll::Ready(Some(_)) = self.as_mut().poll_next_message(cx) {
          break result;
        }

        if let ControlFlow::Break(result) = self.as_mut().poll_next_handle(cx) {
          break result;
        }
      }
    }
  }
}

/// Update message from [`BackgroundTasks`]
#[derive(Debug)]
pub enum TaskUpdate<MessageOut, Output> {
  /// Task sent a message via [`MessageBus`]
  Message(MessageOut),
  /// Task exited
  Finished(Output),
}

/// [`Sender`] for task's inbound messages.
#[derive(Clone, Debug)]
pub struct TaskSender<T: BackgroundTask>(Sender<T::MessageIn>);

impl<T: BackgroundTask> Deref for TaskSender<T> {
  type Target = Sender<T::MessageIn>;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl<T: BackgroundTask> DerefMut for TaskSender<T> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}
