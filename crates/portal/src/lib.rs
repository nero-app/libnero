use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use tokio::sync::{Notify, watch};
use wasmtime::component::{HasData, Resource, ResourceTable, ResourceTableError};
use wasmtime_wasi::p2::{DynPollable, Pollable, subscribe};

pub use self::generated::nero::*;

mod generated {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "nero:portal/imports",
        imports: { default: async | trappable },
        with: {
            "wasi:io": wasmtime_wasi::p2::bindings::io,
            "wasi:clocks": wasmtime_wasi::p2::bindings::clocks,
            "nero:portal/interaction/progress": crate::Progress,
            "nero:portal/callback/listener": crate::Listener,
        },
        trappable_error_type: {
            "nero:portal/callback/listen-error" => crate::ListenError,
        },
    });
}

pub use self::portal::interaction::{ChoiceItem, SelectionRequest, Severity, TextRequest};

#[async_trait::async_trait]
pub trait Interaction: Send + Sync {
    async fn request_text(&self, request: TextRequest) -> Option<String>;

    async fn request_selection(&self, request: SelectionRequest) -> Option<Vec<u32>>;

    fn notify(&self, severity: Severity, message: String, detail: Option<String>);

    fn begin_progress(
        &self,
        title: String,
        cancellable: bool,
        cancel: Cancel,
    ) -> Box<dyn ProgressHandle>;
}

pub trait ProgressHandle: Send + Sync {
    fn report(&self, message: Option<String>, percent: Option<u8>);
}

#[async_trait::async_trait]
pub trait Opener: Send + Sync {
    async fn open(&self, uri: &str) -> bool;
}

pub trait CallbackTransport: Send + Sync {
    fn register(&self, delivery: Delivery) -> Result<String, ListenError>;
}

#[derive(Default)]
struct CancelState {
    flagged: AtomicBool,
    notify: Notify,
}

#[derive(Clone)]
pub struct Cancel(Arc<CancelState>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.flagged.store(true, Ordering::SeqCst);
        self.0.notify.notify_waiters();
    }
}

#[derive(Debug)]
pub enum ListenError {
    Unsupported,
    Other(String),
}

impl From<ResourceTableError> for ListenError {
    fn from(err: ResourceTableError) -> Self {
        Self::Other(err.to_string())
    }
}

type Outcome = Option<Result<String, String>>;

pub struct Delivery(watch::Sender<Outcome>);

impl Delivery {
    pub fn deliver(self, uri: String) {
        let _ = self.0.send(Some(Ok(uri)));
    }

    pub fn fail(self, reason: String) {
        let _ = self.0.send(Some(Err(reason)));
    }

    pub fn is_abandoned(&self) -> bool {
        self.0.is_closed()
    }
}

pub struct PortalCtx {
    interaction: Arc<dyn Interaction>,
    opener: Arc<dyn Opener>,
    callbacks: Arc<dyn CallbackTransport>,
}

impl PortalCtx {
    pub fn new(
        interaction: Arc<dyn Interaction>,
        opener: Arc<dyn Opener>,
        callbacks: Arc<dyn CallbackTransport>,
    ) -> Self {
        Self {
            interaction,
            opener,
            callbacks,
        }
    }
}

pub struct Progress {
    handle: Box<dyn ProgressHandle>,
    cancel: Arc<CancelState>,
}

struct Cancellation {
    cancel: Arc<CancelState>,
}

#[async_trait::async_trait]
impl Pollable for Cancellation {
    async fn ready(&mut self) {
        loop {
            if self.cancel.flagged.load(Ordering::SeqCst) {
                return;
            }
            let notified = self.cancel.notify.notified();
            if self.cancel.flagged.load(Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    }
}

pub struct Listener {
    address: String,
    rx: watch::Receiver<Outcome>,
    deadline: Option<Instant>,
    taken: bool,
}

struct Arrival {
    rx: watch::Receiver<Outcome>,
    deadline: Option<Instant>,
}

#[async_trait::async_trait]
impl Pollable for Arrival {
    async fn ready(&mut self) {
        if self.rx.borrow_and_update().is_some() {
            return;
        }
        match self.deadline {
            Some(deadline) => {
                let _ = tokio::time::timeout_at(deadline.into(), self.rx.changed()).await;
            }
            None => {
                let _ = self.rx.changed().await;
            }
        }
    }
}

pub struct Portal<'a> {
    ctx: &'a PortalCtx,
    table: &'a mut ResourceTable,
}

impl<'a> Portal<'a> {
    pub fn new(ctx: &'a PortalCtx, table: &'a mut ResourceTable) -> Self {
        Self { ctx, table }
    }
}

impl portal::interaction::Host for Portal<'_> {
    async fn request_text(&mut self, request: TextRequest) -> Result<Option<String>> {
        Ok(self.ctx.interaction.request_text(request).await)
    }

    async fn request_selection(&mut self, request: SelectionRequest) -> Result<Option<Vec<u32>>> {
        Ok(self.ctx.interaction.request_selection(request).await)
    }

    async fn notify(
        &mut self,
        severity: Severity,
        message: String,
        detail: Option<String>,
    ) -> Result<()> {
        self.ctx.interaction.notify(severity, message, detail);
        Ok(())
    }
}

impl portal::interaction::HostProgress for Portal<'_> {
    async fn new(&mut self, title: String, cancellable: bool) -> Result<Resource<Progress>> {
        let cancel = Arc::new(CancelState::default());
        let handle =
            self.ctx
                .interaction
                .begin_progress(title, cancellable, Cancel(Arc::clone(&cancel)));

        Ok(self.table.push(Progress { handle, cancel })?)
    }

    async fn report(
        &mut self,
        this: Resource<Progress>,
        message: Option<String>,
        percent: Option<u8>,
    ) -> Result<()> {
        self.table.get(&this)?.handle.report(message, percent);
        Ok(())
    }

    async fn subscribe_cancellation(
        &mut self,
        this: Resource<Progress>,
    ) -> Result<Resource<DynPollable>> {
        let cancel = Arc::clone(&self.table.get(&this)?.cancel);
        let cancellation = self.table.push_child(Cancellation { cancel }, &this)?;
        subscribe(self.table, cancellation)
    }

    async fn drop(&mut self, this: Resource<Progress>) -> Result<()> {
        self.table.delete(this)?;
        Ok(())
    }
}

impl portal::opener::Host for Portal<'_> {
    async fn open(&mut self, uri: String) -> Result<bool> {
        Ok(self.ctx.opener.open(&uri).await)
    }
}

impl portal::callback::Host for Portal<'_> {
    fn convert_listen_error(&mut self, err: ListenError) -> Result<portal::callback::ListenError> {
        Ok(match err {
            ListenError::Unsupported => portal::callback::ListenError::Unsupported,
            ListenError::Other(msg) => portal::callback::ListenError::Other(msg),
        })
    }
}

impl portal::callback::HostListener for Portal<'_> {
    async fn listen(&mut self, timeout: u64) -> Result<Resource<Listener>, ListenError> {
        let (tx, rx) = watch::channel(None);
        let address = self.ctx.callbacks.register(Delivery(tx))?;

        Ok(self.table.push(Listener {
            address,
            rx,
            deadline: Instant::now().checked_add(Duration::from_nanos(timeout)),
            taken: false,
        })?)
    }

    async fn address(&mut self, this: Resource<Listener>) -> Result<String> {
        Ok(self.table.get(&this)?.address.clone())
    }

    async fn subscribe(&mut self, this: Resource<Listener>) -> Result<Resource<DynPollable>> {
        let listener = self.table.get(&this)?;
        let arrival = Arrival {
            rx: listener.rx.clone(),
            deadline: listener.deadline,
        };
        let arrival = self.table.push_child(arrival, &this)?;
        subscribe(self.table, arrival)
    }

    async fn get(
        &mut self,
        this: Resource<Listener>,
    ) -> Result<Option<Result<Result<String, portal::callback::ReceiveError>, ()>>> {
        let listener = self.table.get_mut(&this)?;

        if listener.taken {
            return Ok(Some(Err(())));
        }

        let outcome = listener.rx.borrow_and_update().clone();
        if let Some(outcome) = outcome {
            listener.taken = true;
            return Ok(Some(Ok(match outcome {
                Ok(uri) => Ok(uri),
                Err(reason) => Err(portal::callback::ReceiveError::Other(reason)),
            })));
        }

        let abandoned = listener.rx.has_changed().is_err();
        let expired = listener.deadline.is_some_and(|d| Instant::now() >= d);
        if expired {
            listener.taken = true;
            return Ok(Some(Ok(Err(portal::callback::ReceiveError::Expired))));
        }

        if abandoned {
            listener.taken = true;
            return Ok(Some(Ok(Err(portal::callback::ReceiveError::Other(
                "callback transport closed".into(),
            )))));
        }

        Ok(None)
    }

    async fn drop(&mut self, this: Resource<Listener>) -> Result<()> {
        self.table.delete(this)?;
        Ok(())
    }
}

pub trait PortalView {
    fn portal(&mut self) -> Portal<'_>;
}

pub fn add_to_linker<T: PortalView + Send>(l: &mut wasmtime::component::Linker<T>) -> Result<()> {
    portal::interaction::add_to_linker::<T, HasPortal>(l, T::portal)?;
    portal::opener::add_to_linker::<T, HasPortal>(l, T::portal)?;
    portal::callback::add_to_linker::<T, HasPortal>(l, T::portal)?;
    Ok(())
}

struct HasPortal;
impl HasData for HasPortal {
    type Data<'a> = Portal<'a>;
}
