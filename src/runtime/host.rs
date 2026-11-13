//! One worker: a QuickJS engine, the `__host` ops it calls, and the event loop that drives it.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use rquickjs::{
    Context, Ctx, Function, Module, Object, Persistent, Promise, Runtime, TypedArray, Value, promise::PromiseState,
};
use tokio::sync::{mpsc, oneshot};

use super::{Event, Job, Shared, Sink, SocketJob, WsData, WsOut};

pub const HOST_JS: &str = include_str!("js/host.js");

#[cfg_attr(not(feature = "fetch"), allow(dead_code))]
enum OpValue {
    Unit,
    Null,
    Text(String),
    Bytes(Vec<u8>),
}

struct Completion {
    id: f64,
    result: Result<OpValue, String>,
}

/// Tracks how long JS has been running so the interrupt handler can stop runaway code.
struct Clock {
    slice: Cell<Option<Instant>>,
    pump: Cell<Option<Instant>>,
    cpu: Duration,
    pump_budget: Duration,
    tripped: Cell<bool>,
}

impl Clock {
    fn begin_slice(&self) {
        self.slice.set(Some(Instant::now()));
    }
    fn over(&self) -> bool {
        let now = Instant::now();
        let slice = self.slice.get().is_some_and(|t| now - t > self.cpu);
        let pump = self.pump.get().is_some_and(|t| now - t > self.pump_budget);
        if slice || pump {
            self.tripped.set(true);
        }
        slice || pump
    }
}

struct HostState {
    shared: Arc<Shared>,
    inflight: Arc<AtomicUsize>,
    /// This worker's index in the pool (owns the sockets routed to it).
    worker: usize,
    requests: RefCell<HashMap<u64, Sink>>,
    /// Async op ids JS gave up on (aborted fetches): their results are dropped.
    cancelled: RefCell<HashSet<u64>>,
    #[cfg(feature = "fetch")]
    bodies: RefCell<HashMap<u32, reqwest::Response>>,
    #[cfg(feature = "fetch")]
    next_handle: Cell<u32>,
    comp: mpsc::UnboundedSender<Completion>,
    started: Instant,
}

impl HostState {
    fn complete(&self, id: f64, result: Result<OpValue, String>) {
        let _ = self.comp.send(Completion { id, result });
    }
    fn sink(&self, req: f64) -> Option<Sink> {
        self.requests.borrow().get(&(req as u64)).cloned()
    }
    #[cfg(feature = "fetch")]
    fn take_cancelled(&self, op: f64) -> bool {
        self.cancelled.borrow_mut().remove(&(op as u64))
    }
}

pub(super) fn run_worker(
    index: usize,
    shared: Arc<Shared>,
    jobs: mpsc::UnboundedReceiver<Job>,
    inflight: Arc<AtomicUsize>,
    ready: oneshot::Sender<Result<(), String>>,
) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("worker event loop");
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, worker_loop(index, shared, jobs, inflight, ready));
}

async fn worker_loop(
    index: usize,
    shared: Arc<Shared>,
    mut jobs: mpsc::UnboundedReceiver<Job>,
    inflight: Arc<AtomicUsize>,
    ready: oneshot::Sender<Result<(), String>>,
) {
    let mut ready = Some(ready);
    loop {
        let mut engine = match Engine::new(index, shared.clone(), inflight.clone()).await {
            Ok(engine) => {
                if let Some(r) = ready.take() {
                    let _ = r.send(Ok(()));
                }
                engine
            }
            Err(e) => {
                if let Some(r) = ready.take() {
                    let _ = r.send(Err(e));
                    return;
                }
                eprintln!("[forge] JS worker failed to restart: {e}");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        match engine.run(&mut jobs).await {
            None => return,
            Some(reason) => eprintln!("[forge] recycling JS worker: {reason}"),
        }
    }
}

struct Engine {
    // Field order matters: the context must drop before the runtime.
    ctx: Context,
    rt: Runtime,
    st: Rc<HostState>,
    clock: Rc<Clock>,
    comp_rx: mpsc::UnboundedReceiver<Completion>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Pending responses on this engine end now (their clients see a 500 or a cut stream).
        let n = self.st.requests.borrow_mut().drain().count();
        self.st.inflight.fetch_sub(n, Ordering::AcqRel);
        // Socket state lived in this engine; clients reconnect to the replacement.
        self.st.shared.sockets.close_all(Some(self.st.worker), 1012, "worker restarted");
    }
}

impl Engine {
    async fn new(worker: usize, shared: Arc<Shared>, inflight: Arc<AtomicUsize>) -> Result<Self, String> {
        let rt = Runtime::new().map_err(|e| e.to_string())?;
        if shared.limits.memory_bytes > 0 {
            rt.set_memory_limit(shared.limits.memory_bytes);
        }
        rt.set_max_stack_size(8 << 20);
        let clock = Rc::new(Clock {
            slice: Cell::new(None),
            pump: Cell::new(None),
            cpu: shared.limits.cpu.max(Duration::from_millis(10)),
            pump_budget: (shared.limits.cpu * 5).max(shared.limits.timeout),
            tripped: Cell::new(false),
        });
        let c = clock.clone();
        rt.set_interrupt_handler(Some(Box::new(move || c.over())));
        let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
        let (comp, comp_rx) = mpsc::unbounded_channel();
        let st = Rc::new(HostState {
            shared: shared.clone(),
            inflight,
            worker,
            requests: RefCell::new(HashMap::new()),
            cancelled: RefCell::new(HashSet::new()),
            #[cfg(feature = "fetch")]
            bodies: RefCell::new(HashMap::new()),
            #[cfg(feature = "fetch")]
            next_handle: Cell::new(0),
            comp,
            started: Instant::now(),
        });
        let mut engine = Engine { ctx, rt, st, clock, comp_rx };

        let promise = engine.enter(|ctx, st| {
            install(&ctx, st).map_err(|e| describe(&ctx, e))?;
            ctx.eval::<(), _>(HOST_JS).map_err(|e| describe(&ctx, e))?;
            let p =
                Module::evaluate(ctx.clone(), "forge:server", shared.code.as_bytes()).map_err(|e| describe(&ctx, e))?;
            Ok(Persistent::save(&ctx, p))
        })?;

        let deadline = Instant::now() + shared.limits.timeout.max(Duration::from_secs(5));
        loop {
            engine.pump()?;
            let state = engine.ctx.with(|ctx| {
                let p: Promise = promise.clone().restore(&ctx).map_err(|e| e.to_string())?;
                Ok::<_, String>(match p.state() {
                    PromiseState::Pending => None,
                    PromiseState::Resolved => Some(Ok(())),
                    PromiseState::Rejected => Some(Err(match p.result::<Value>() {
                        Some(Err(e)) => describe(&ctx, e),
                        _ => "server bundle failed to load".to_string(),
                    })),
                })
            })?;
            match state {
                Some(Ok(())) => break,
                Some(Err(e)) => return Err(e),
                None => match tokio::time::timeout_at(deadline.into(), engine.comp_rx.recv()).await {
                    Ok(Some(c)) => engine.resolve(c)?,
                    _ => return Err("server bundle did not finish loading (top-level await never settled)".into()),
                },
            }
        }
        drop(promise);
        let ok = engine.ctx.with(|ctx| ctx.globals().get::<_, Function>("__forge_dispatch").is_ok());
        if !ok {
            return Err("server bundle did not register a request handler".into());
        }
        Ok(engine)
    }

    /// Run JS with the CPU clock armed.
    fn enter<R>(&self, f: impl FnOnce(Ctx<'_>, &Rc<HostState>) -> Result<R, String>) -> Result<R, String> {
        self.clock.begin_slice();
        let out = self.ctx.with(|ctx| f(ctx, &self.st));
        self.clock.slice.set(None);
        if self.clock.tripped.get() {
            return Err(format!("JS ran longer than the CPU budget ({:?})", self.clock.cpu));
        }
        out
    }

    /// `None` when the pool shuts down, `Some(reason)` when this engine must be replaced.
    async fn run(&mut self, jobs: &mut mpsc::UnboundedReceiver<Job>) -> Option<String> {
        loop {
            let step = tokio::select! {
                biased;
                Some(c) = self.comp_rx.recv() => self.resolve(c),
                job = jobs.recv() => match job {
                    Some(job) => self.dispatch(job),
                    None => return None,
                },
            };
            let step = step.and_then(|_| {
                while let Ok(c) = self.comp_rx.try_recv() {
                    self.resolve(c)?;
                }
                self.pump()
            });
            if let Err(reason) = step {
                return Some(reason);
            }
            if self.st.requests.borrow().is_empty() {
                let usage = self.rt.memory_usage();
                let limit = self.st.shared.limits.memory_bytes as i64;
                if limit > 0 && usage.malloc_size > limit / 2 {
                    self.rt.run_gc();
                    if self.rt.memory_usage().malloc_size > limit * 3 / 4 {
                        return Some("heap stayed above 75% of memoryLimitMb while idle".into());
                    }
                }
            }
        }
    }

    fn dispatch(&mut self, job: Job) -> Result<(), String> {
        match job {
            Job::Request { id, request, body, sink } => self.request(id, request, body, sink),
            Job::Abort(id) => {
                if self.st.requests.borrow_mut().remove(&id).is_none() {
                    return Ok(()); // already finished
                }
                self.st.inflight.fetch_sub(1, Ordering::AcqRel);
                self.enter(|ctx, _| {
                    let f: Function = ctx.globals().get("__forge_abort").map_err(|e| describe(&ctx, e))?;
                    f.call::<_, ()>((id as f64,)).map_err(|e| describe(&ctx, e))
                })
            }
            Job::Socket(job) => {
                let (kind, sid) = match &job {
                    SocketJob::Open { sid, .. } => ("open", *sid),
                    SocketJob::Message { sid, .. } => ("message", *sid),
                    SocketJob::Close { sid, .. } => ("close", *sid),
                };
                let result = self.enter(|ctx, _| {
                    let f: Function = ctx.globals().get("__forge_socket").map_err(|e| describe(&ctx, e))?;
                    let text = |s: &str| rquickjs::String::from_str(ctx.clone(), s).map(|s| s.into_value());
                    let (a, b): (rquickjs::Result<Value>, rquickjs::Result<Value>) = match &job {
                        SocketJob::Open { request, .. } => (text(request), Ok(Value::new_undefined(ctx.clone()))),
                        SocketJob::Message { data: WsData::Text(t), .. } => {
                            (text(t), Ok(Value::new_bool(ctx.clone(), false)))
                        }
                        SocketJob::Message { data: WsData::Binary(b), .. } => (
                            TypedArray::<u8>::new(ctx.clone(), b.to_vec()).map(|a| a.into_value()),
                            Ok(Value::new_bool(ctx.clone(), true)),
                        ),
                        SocketJob::Close { code, reason, .. } => {
                            (Ok(Value::new_int(ctx.clone(), *code as i32)), text(reason))
                        }
                    };
                    let (a, b) = (a.map_err(|e| describe(&ctx, e))?, b.map_err(|e| describe(&ctx, e))?);
                    f.call::<_, ()>((kind, sid as f64, a, b)).map_err(|e| describe(&ctx, e))
                });
                match result {
                    Err(e) if !self.clock.tripped.get() => {
                        eprintln!("[forge] socket {kind} failed: {e}");
                        self.st.shared.sockets.send(sid, WsOut::Close(1011, "server error".into()));
                        Ok(())
                    }
                    other => other,
                }
            }
        }
    }

    fn request(&mut self, id: u64, request: String, body: Option<Bytes>, sink: Sink) -> Result<(), String> {
        self.st.requests.borrow_mut().insert(id, sink);
        self.st.inflight.fetch_add(1, Ordering::AcqRel);
        let result = self.enter(|ctx, _| {
            let f: Function = ctx.globals().get("__forge_dispatch").map_err(|e| describe(&ctx, e))?;
            let body: Value = match body {
                Some(b) => TypedArray::<u8>::new(ctx.clone(), b.to_vec()).map_err(|e| describe(&ctx, e))?.into_value(),
                None => Value::new_null(ctx.clone()),
            };
            f.call::<_, ()>((id as f64, request, body)).map_err(|e| describe(&ctx, e))
        });
        if let Err(e) = &result {
            if !self.clock.tripped.get() {
                eprintln!("[forge] dispatch failed: {e}");
                if self.st.requests.borrow_mut().remove(&id).is_some() {
                    self.st.inflight.fetch_sub(1, Ordering::AcqRel);
                }
                return Ok(());
            }
        }
        result
    }

    fn resolve(&mut self, c: Completion) -> Result<(), String> {
        self.enter(|ctx, _| {
            let f: Function = ctx.globals().get("__forge_resolve").map_err(|e| describe(&ctx, e))?;
            let (ok, value): (bool, Value) = match c.result {
                Ok(OpValue::Unit) => (true, Value::new_undefined(ctx.clone())),
                Ok(OpValue::Null) => (true, Value::new_null(ctx.clone())),
                Ok(OpValue::Text(s)) => {
                    (true, rquickjs::String::from_str(ctx.clone(), &s).map_err(|e| describe(&ctx, e))?.into_value())
                }
                Ok(OpValue::Bytes(b)) => {
                    (true, TypedArray::<u8>::new(ctx.clone(), b).map_err(|e| describe(&ctx, e))?.into_value())
                }
                Err(e) => {
                    (false, rquickjs::String::from_str(ctx.clone(), &e).map_err(|e| describe(&ctx, e))?.into_value())
                }
            };
            f.call::<_, ()>((c.id, ok, value)).map_err(|e| describe(&ctx, e))
        })
    }

    /// Run microtasks and macrotasks until JS is idle.
    fn pump(&mut self) -> Result<(), String> {
        self.clock.pump.set(Some(Instant::now()));
        let result = (|| loop {
            loop {
                self.clock.begin_slice();
                match self.rt.execute_pending_job() {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => {
                        let msg = e.0.with(|ctx| describe_value(&ctx, ctx.catch()));
                        if self.clock.tripped.get() || msg.contains("out of memory") {
                            return Err(msg);
                        }
                        eprintln!("[forge] uncaught: {msg}");
                    }
                }
            }
            self.clock.begin_slice();
            let ticked = self.ctx.with(|ctx| {
                let f: Function = ctx.globals().get("__forge_tick").map_err(|e| describe(&ctx, e))?;
                f.call::<_, bool>(()).map_err(|e| describe(&ctx, e))
            });
            if self.clock.tripped.get() {
                return Err(format!("JS ran longer than the CPU budget ({:?})", self.clock.cpu));
            }
            match ticked {
                Ok(true) => continue,
                Ok(false) if self.rt.is_job_pending() => continue,
                Ok(false) => return Ok(()),
                Err(e) if e.contains("out of memory") => return Err(e),
                Err(e) => {
                    eprintln!("[forge] {e}");
                    return Ok(());
                }
            }
        })();
        self.clock.slice.set(None);
        self.clock.pump.set(None);
        result
    }
}

fn describe(ctx: &Ctx<'_>, e: rquickjs::Error) -> String {
    match e {
        rquickjs::Error::Exception => describe_value(ctx, ctx.catch()),
        other => other.to_string(),
    }
}

fn describe_value(_ctx: &Ctx<'_>, v: Value<'_>) -> String {
    if let Some(ex) = v.as_exception() {
        let msg = ex.message().unwrap_or_default();
        let stack = ex.stack().unwrap_or_default();
        return if stack.is_empty() { msg } else { format!("{msg}\n{}", stack.trim_end()) };
    }
    if let Some(s) = v.as_string() {
        return s.to_string().unwrap_or_default();
    }
    format!("{v:?}")
}

fn is_nullish(v: &Value<'_>) -> bool {
    v.is_null() || v.is_undefined()
}

fn bytes_of(v: &Value<'_>) -> Option<Vec<u8>> {
    if let Some(s) = v.as_string() {
        return s.to_string().ok().map(String::into_bytes);
    }
    let arr = TypedArray::<u8>::from_value(v.clone()).ok()?;
    // SAFETY: copied out immediately, before any JS can run.
    unsafe { arr.as_bytes() }.map(<[u8]>::to_vec)
}

fn new_bytes<'js>(ctx: &Ctx<'js>, bytes: Vec<u8>) -> rquickjs::Result<TypedArray<'js, u8>> {
    TypedArray::new(ctx.clone(), bytes)
}

/// `[href, protocol, username, password, host, hostname, port, pathname, search, hash, origin]`
fn parse_url(input: &str, base: Option<&str>) -> Option<Vec<String>> {
    let url = match base {
        Some(b) => url::Url::parse(b).ok()?.join(input).ok()?,
        None => url::Url::parse(input).ok()?,
    };
    let host = url.host_str().map(|h| match url.port() {
        Some(p) => format!("{h}:{p}"),
        None => h.to_string(),
    });
    Some(vec![
        url.as_str().to_string(),
        format!("{}:", url.scheme()),
        url.username().to_string(),
        url.password().unwrap_or_default().to_string(),
        host.unwrap_or_default(),
        url.host_str().unwrap_or_default().to_string(),
        url.port().map(|p| p.to_string()).unwrap_or_default(),
        url.path().to_string(),
        url.query().map(|q| format!("?{q}")).unwrap_or_default(),
        url.fragment().map(|f| format!("#{f}")).unwrap_or_default(),
        url.origin().ascii_serialization(),
    ])
}

fn install<'js>(ctx: &Ctx<'js>, st: &Rc<HostState>) -> rquickjs::Result<()> {
    let host = Object::new(ctx.clone())?;

    host.set(
        "log",
        Function::new(ctx.clone(), move |level: i32, msg: String| match level {
            0 => println!("{msg}"),
            1 => eprintln!("warn: {msg}"),
            _ => eprintln!("error: {msg}"),
        })?,
    )?;
    let s = st.clone();
    host.set("now", Function::new(ctx.clone(), move || s.started.elapsed().as_secs_f64() * 1000.0)?)?;
    host.set(
        "random",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, n: usize| {
            let mut buf = vec![0u8; n.min(65_536)];
            getrandom::fill(&mut buf).map_err(|e| rquickjs::Exception::throw_internal(&ctx, &e.to_string()))?;
            new_bytes(&ctx, buf)
        })?,
    )?;
    host.set("encode", Function::new(ctx.clone(), move |ctx: Ctx<'js>, s: String| new_bytes(&ctx, s.into_bytes()))?)?;
    host.set(
        "decode",
        Function::new(ctx.clone(), move |v: Value<'js>| {
            String::from_utf8_lossy(&bytes_of(&v).unwrap_or_default()).into_owned()
        })?,
    )?;
    host.set(
        "url",
        Function::new(ctx.clone(), move |input: String, base: Value<'js>| {
            let base = if is_nullish(&base) { None } else { base.as_string().and_then(|s| s.to_string().ok()) };
            parse_url(&input, base.as_deref())
        })?,
    )?;
    let s = st.clone();
    host.set("env", Function::new(ctx.clone(), move |key: String| s.shared.env.get(&key).cloned())?)?;

    let s = st.clone();
    host.set(
        "timer",
        Function::new(ctx.clone(), move |id: f64, ms: f64| {
            let s = s.clone();
            tokio::task::spawn_local(async move {
                tokio::time::sleep(Duration::from_secs_f64(ms.max(0.0) / 1000.0)).await;
                s.complete(id, Ok(OpValue::Unit));
            });
        })?,
    )?;

    // ---- response sinks ----
    let s = st.clone();
    host.set(
        "head",
        Function::new(ctx.clone(), move |req: f64, json: String| s.sink(req).map_or(2, |k| k.send(Event::Head(json))))?,
    )?;
    let s = st.clone();
    host.set(
        "write",
        Function::new(ctx.clone(), move |req: f64, chunk: Value<'js>| {
            let Some(sink) = s.sink(req) else { return 2 };
            match bytes_of(&chunk) {
                Some(b) => sink.send(Event::Data(Bytes::from(b))),
                None => 0,
            }
        })?,
    )?;
    let s = st.clone();
    host.set(
        "drain",
        Function::new(ctx.clone(), move |id: f64, req: f64| {
            let s = s.clone();
            match s.sink(req) {
                None => s.complete(id, Ok(OpValue::Unit)),
                Some(sink) => {
                    tokio::task::spawn_local(async move {
                        sink.drained().await;
                        s.complete(id, Ok(OpValue::Unit));
                    });
                }
            }
        })?,
    )?;
    let s = st.clone();
    host.set(
        "end",
        Function::new(ctx.clone(), move |req: f64| {
            if s.requests.borrow_mut().remove(&(req as u64)).is_some() {
                s.inflight.fetch_sub(1, Ordering::AcqRel);
            }
        })?,
    )?;

    // ---- shared cache ----
    let s = st.clone();
    host.set("cacheGet", Function::new(ctx.clone(), move |key: String| s.shared.cache.lock().unwrap().get(&key))?)?;
    let s = st.clone();
    host.set(
        "cacheKeys",
        Function::new(ctx.clone(), move |prefix: String| s.shared.cache.lock().unwrap().keys(&prefix))?,
    )?;
    let s = st.clone();
    host.set(
        "cacheSet",
        Function::new(ctx.clone(), move |key: String, value: String, ttl_ms: f64, tags: String| {
            let tags = tags.split(',').filter(|t| !t.is_empty()).map(str::to_string).collect();
            s.shared.cache.lock().unwrap().set(key, value, Duration::from_secs_f64(ttl_ms.max(0.0) / 1000.0), tags);
        })?,
    )?;
    let s = st.clone();
    host.set(
        "cacheLease",
        Function::new(ctx.clone(), move |key: String, ttl_ms: f64| {
            s.shared.cache.lock().unwrap().lease(&key, Duration::from_secs_f64(ttl_ms.max(0.0) / 1000.0))
        })?,
    )?;
    let s = st.clone();
    host.set(
        "cacheRelease",
        Function::new(ctx.clone(), move |key: String| s.shared.cache.lock().unwrap().release(&key))?,
    )?;
    let s = st.clone();
    host.set(
        "cacheDelete",
        Function::new(ctx.clone(), move |key: String| {
            s.shared.cache.lock().unwrap().invalidate(&key);
            if let Some(hook) = s.shared.on_invalidate.lock().unwrap().as_ref() {
                hook(&key);
            }
        })?,
    )?;

    // ---- WebSockets and pub/sub ----
    fn ws_data(v: &Value<'_>) -> Option<WsData> {
        if let Some(s) = v.as_string() {
            return s.to_string().ok().map(WsData::Text);
        }
        bytes_of(v).map(|b| WsData::Binary(Bytes::from(b)))
    }
    let s = st.clone();
    host.set(
        "wsSend",
        Function::new(ctx.clone(), move |sid: f64, data: Value<'js>| match ws_data(&data) {
            Some(d) => s.shared.sockets.send(sid as u64, WsOut::Data(d)),
            None => false,
        })?,
    )?;
    let s = st.clone();
    host.set(
        "wsClose",
        Function::new(ctx.clone(), move |sid: f64, code: f64, reason: String| {
            s.shared.sockets.send(sid as u64, WsOut::Close(code as u16, reason))
        })?,
    )?;
    let s = st.clone();
    host.set(
        "subscribe",
        Function::new(ctx.clone(), move |sid: f64, topic: String| s.shared.sockets.subscribe(sid as u64, &topic))?,
    )?;
    let s = st.clone();
    host.set(
        "unsubscribe",
        Function::new(ctx.clone(), move |sid: f64, topic: String| s.shared.sockets.unsubscribe(sid as u64, &topic))?,
    )?;
    let s = st.clone();
    host.set(
        "publish",
        Function::new(ctx.clone(), move |topic: String, data: Value<'js>, except: Value<'js>| {
            let except = except.as_number().map(|n| n as u64);
            ws_data(&data).map_or(0, |d| s.shared.sockets.publish(&topic, d, except))
        })?,
    )?;
    let s = st.clone();
    host.set("subscribers", Function::new(ctx.clone(), move |topic: String| s.shared.sockets.subscribers(&topic))?)?;
    let s = st.clone();
    host.set(
        "cancel",
        Function::new(ctx.clone(), move |op: f64| {
            s.cancelled.borrow_mut().insert(op as u64);
        })?,
    )?;

    install_fetch(ctx, &host, st)?;
    ctx.globals().set("__host", host)?;
    Ok(())
}

#[cfg(not(feature = "fetch"))]
fn install_fetch<'js>(ctx: &Ctx<'js>, host: &Object<'js>, st: &Rc<HostState>) -> rquickjs::Result<()> {
    let s = st.clone();
    host.set(
        "fetch",
        Function::new(ctx.clone(), move |id: f64, _meta: String, _body: Value<'js>| {
            s.complete(id, Err("fetch is disabled (built without the `fetch` feature)".into()))
        })?,
    )?;
    for name in ["read", "drop", "pipe"] {
        host.set(name, Function::new(ctx.clone(), || ())?)?;
    }
    Ok(())
}

#[cfg(feature = "fetch")]
fn install_fetch<'js>(ctx: &Ctx<'js>, host: &Object<'js>, st: &Rc<HostState>) -> rquickjs::Result<()> {
    #[derive(serde::Deserialize)]
    struct Meta {
        url: String,
        method: String,
        headers: Vec<(String, String)>,
    }

    let s = st.clone();
    host.set(
        "fetch",
        Function::new(ctx.clone(), move |id: f64, meta: String, body: Value<'js>| {
            let s = s.clone();
            let body = if is_nullish(&body) { None } else { bytes_of(&body) };
            let prepared = (|| {
                let meta: Meta = serde_json::from_str(&meta).map_err(|e| e.to_string())?;
                let url = url::Url::parse(&meta.url).map_err(|e| format!("invalid URL {}: {e}", meta.url))?;
                if !matches!(url.scheme(), "http" | "https") {
                    return Err(format!("fetch: unsupported scheme `{}`", url.scheme()));
                }
                let host = url.host_str().unwrap_or_default();
                if !s.shared.fetch_allowed(host) {
                    return Err(format!(
                        "fetch to `{host}` is not allowed; add it to server.allowFetch in forge.config.json"
                    ));
                }
                let method = reqwest::Method::from_bytes(meta.method.as_bytes()).map_err(|e| e.to_string())?;
                let mut req = s.shared.http.request(method, url);
                for (k, v) in meta.headers {
                    req = req.header(k, v);
                }
                if let Some(b) = body {
                    req = req.body(b);
                }
                Ok(req)
            })();
            let req = match prepared {
                Ok(r) => r,
                Err(e) => return s.complete(id, Err(e)),
            };
            tokio::task::spawn_local(async move {
                match req.send().await {
                    Ok(resp) => {
                        let handle = s.next_handle.get().wrapping_add(1);
                        s.next_handle.set(handle);
                        let headers: Vec<(String, String)> = resp
                            .headers()
                            .iter()
                            .map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
                            .collect();
                        let meta = serde_json::json!([
                            resp.status().as_u16(),
                            resp.status().canonical_reason().unwrap_or(""),
                            headers,
                            handle,
                            resp.url().as_str()
                        ]);
                        if s.take_cancelled(id) {
                            return;
                        }
                        s.bodies.borrow_mut().insert(handle, resp);
                        s.complete(id, Ok(OpValue::Text(meta.to_string())));
                    }
                    Err(e) => s.complete(id, Err(format!("fetch failed: {e}"))),
                }
            });
        })?,
    )?;

    let s = st.clone();
    host.set(
        "read",
        Function::new(ctx.clone(), move |id: f64, handle: u32| {
            let s = s.clone();
            let Some(mut resp) = s.bodies.borrow_mut().remove(&handle) else {
                return s.complete(id, Ok(OpValue::Null));
            };
            tokio::task::spawn_local(async move {
                match resp.chunk().await {
                    Ok(Some(chunk)) => {
                        s.bodies.borrow_mut().insert(handle, resp);
                        s.complete(id, Ok(OpValue::Bytes(chunk.to_vec())));
                    }
                    Ok(None) => s.complete(id, Ok(OpValue::Null)),
                    Err(e) => s.complete(id, Err(format!("reading response body: {e}"))),
                }
            });
        })?,
    )?;

    let s = st.clone();
    host.set(
        "drop",
        Function::new(ctx.clone(), move |handle: u32| {
            s.bodies.borrow_mut().remove(&handle);
        })?,
    )?;

    // Stream an untouched upstream body straight to the client, without entering JS per chunk.
    let s = st.clone();
    host.set(
        "pipe",
        Function::new(ctx.clone(), move |req: f64, handle: u32| {
            let (Some(sink), Some(mut resp)) = (s.sink(req), s.bodies.borrow_mut().remove(&handle)) else { return };
            tokio::task::spawn_local(async move {
                while let Ok(Some(chunk)) = resp.chunk().await {
                    match sink.send(Event::Data(chunk)) {
                        0 => {}
                        1 => sink.drained().await,
                        _ => break,
                    }
                }
            });
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_parts() {
        let p = parse_url("/a/b?x=1#h", Some("http://localhost:3000/z")).unwrap();
        assert_eq!(
            p,
            [
                "http://localhost:3000/a/b?x=1#h",
                "http:",
                "",
                "",
                "localhost:3000",
                "localhost",
                "3000",
                "/a/b",
                "?x=1",
                "#h",
                "http://localhost:3000"
            ]
        );
        assert!(parse_url("not a url", None).is_none());
    }
}
