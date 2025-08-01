use core::fmt;

use async_channel::{Receiver, Sender};

/// Main wrapper trait for starting and handling Tasks
pub trait BackgroundTask {
  /// The actual "Task" handle, this should something like `async_executer::Task` or `tokio::task::JoinHandle`
  type Task: Future;

  /// Incoming message type
  type MessageIn;

  /// Outgoing message type
  type MessageOut;

  /// Start the task with the provided [`MessageBus`]
  fn run(self, message_bus: MessageBus<Self::MessageIn, Self::MessageOut>) -> Self::Task;
}

/// Combined [`Receiver`] and [`Sender`] for inbound and outbound messages respectivly.
#[derive(Clone)]
pub struct MessageBus<MessageIn, MessageOut> {
  tx: Sender<MessageOut>,
  rx: Receiver<MessageIn>,
}

impl<MessageIn, MessageOut> fmt::Debug for MessageBus<MessageIn, MessageOut> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("MessageBus")
      .field("tx", &self.tx)
      .field("rx", &self.rx)
      .finish()
  }
}

impl<MessageIn, MessageOut> MessageBus<MessageIn, MessageOut> {
  /// Create new bus from sender and reciver
  pub(crate) fn from_parts(tx: Sender<MessageOut>, rx: Receiver<MessageIn>) -> Self {
    MessageBus { tx, rx }
  }

  /// Split the bus to it's parts
  pub fn split(self) -> (Sender<MessageOut>, Receiver<MessageIn>) {
    let MessageBus { tx, rx } = self;
    (tx, rx)
  }

  /// Create [`Send`](async_channel::Send) future to send a message to bus.
  pub fn send(&self, message: MessageOut) -> async_channel::Send<'_, MessageOut> {
    self.tx.send(message)
  }

  /// Create [`Recv`](async_channel::Recv) future to consume a message from message bus.
  pub fn recv(&self) -> async_channel::Recv<'_, MessageIn> {
    self.rx.recv()
  }
}
