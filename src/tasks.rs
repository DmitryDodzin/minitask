use core::{
  ops::{ControlFlow, Deref, DerefMut},
  pin::Pin,
  task::{Context, Poll},
};
use std::collections::VecDeque;

use async_channel::{Receiver, Sender};
use futures_core::{Stream, ready};
use pin_project_lite::pin_project;

use crate::{BackgroundTask, MessageBus, future::FuturesMap, stream::StreamMap};

pin_project! {
  /// Map to register [`BackgroundTask`] and [`Stream`] the [`TaskUpdate`]s form said tasks.
  ///
  /// Note: it will also try and drain any remaining messages on channel to yield them before `TaskUpdate::Finished`.
  pub struct BackgroundTasks<K, MessageOut, Error, Task> {
    #[pin]
    streams: StreamMap<K, Pin<Box<Receiver<MessageOut>>>>,

    #[pin]
    handles: FuturesMap<K, Task>,

    task_drain_queue: VecDeque<(K, TaskUpdate<MessageOut, Error>)>,
  }
}

impl<K, MessageOut, Error, Task> BackgroundTasks<K, MessageOut, Error, Task> {
  pub fn new() -> Self {
    BackgroundTasks {
      streams: StreamMap::new(),
      handles: FuturesMap::new(),
      task_drain_queue: VecDeque::new(),
    }
  }

  pub fn with_capacity(capacity: usize) -> Self {
    BackgroundTasks {
      streams: StreamMap::with_capacity(capacity),
      handles: FuturesMap::with_capacity(capacity),
      task_drain_queue: VecDeque::new(),
    }
  }

  /// Register new [`BackgroundTask`] with a specific key, this key is used to sync up the message
  /// bus channels and started task.
  pub fn register<T>(&mut self, key: K, task: T) -> TaskSender<T>
  where
    K: Clone + PartialEq,
    T: BackgroundTask<Error = Error, MessageOut = MessageOut, Task = Task>,
  {
    let (in_tx, in_rx) = async_channel::unbounded::<T::MessageIn>();
    let (out_tx, out_rx) = async_channel::unbounded::<T::MessageOut>();

    let message_bus = MessageBus::from_parts(out_tx, in_rx);
    self.streams.insert(key.clone(), Box::pin(out_rx));
    self.handles.insert(key, task.run(message_bus));

    TaskSender(in_tx)
  }
}

impl<K, MessageOut, Error, Task> BackgroundTasks<K, MessageOut, Error, Task>
where
  K: Clone + PartialEq,
  Task: Future<Output = Result<(), Error>> + Unpin,
{
  fn poll_next_drain_queue(
    self: Pin<&mut Self>,
  ) -> ControlFlow<Poll<Option<(K, TaskUpdate<MessageOut, Error>)>>> {
    if let result @ Some(_) = self.project().task_drain_queue.pop_front() {
      ControlFlow::Break(Poll::Ready(result))
    } else {
      ControlFlow::Continue(())
    }
  }

  fn poll_next_message(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
  ) -> Poll<Option<(K, TaskUpdate<MessageOut, Error>)>> {
    let next = ready!(self.project().streams.poll_next(cx));
    Poll::Ready(next.map(|(key, message)| (key, TaskUpdate::Message(message))))
  }

  fn poll_next_handle(
    self: Pin<&mut Self>,
    cx: &mut Context<'_>,
  ) -> ControlFlow<Poll<Option<(K, TaskUpdate<MessageOut, Error>)>>> {
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

impl<K, MessageOut, Error, Task> Default for BackgroundTasks<K, MessageOut, Error, Task> {
  fn default() -> Self {
    Self::new()
  }
}

impl<K, MessageOut, Error, Task> Stream for BackgroundTasks<K, MessageOut, Error, Task>
where
  K: Clone + PartialEq,
  Task: Future<Output = Result<(), Error>> + Unpin,
{
  type Item = (K, TaskUpdate<MessageOut, Error>);

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    loop {
      if let Some(result) = self.as_mut().poll_next_drain_queue().break_value() {
        break result;
      }

      if fastrand::bool() {
        match self.as_mut().poll_next_handle(cx) {
          ControlFlow::Continue(()) => continue,
          ControlFlow::Break(result @ Poll::Ready(Some(_))) => break result,
          ControlFlow::Break(_) => {}
        }

        break self.as_mut().poll_next_message(cx);
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
pub enum TaskUpdate<MessageOut, Error> {
  /// Task sent a message via [`MessageBus`]
  Message(MessageOut),
  /// Task exited
  Finished(Result<(), Error>),
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
