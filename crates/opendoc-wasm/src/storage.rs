//! Durable browser storage: IndexedDB behind the synchronous Rust store traits.
//!
//! The whole asynchrony problem lives in this file. `opendoc-store`'s
//! [`MirroredVolume`] is the synchronous store of record that
//! `OpenDocApp::dispatch_command` reads and writes; everything here is the
//! driver that carries its mutations into IndexedDB and reports back how far
//! durability has reached. Nothing blocks: there is no synchronous IndexedDB
//! API, and a browser's main thread cannot be blocked on one anyway (see
//! `docs/adr/0008-browser-storage-adapter.md`).
//!
//! Two guarantees this driver owes the volume:
//!
//! * **One batch, one transaction.** Every mutation in a drained batch is
//!   written in a single IndexedDB `readwrite` transaction, so a crash leaves
//!   the durable store at a batch boundary — never halfway through one. Since
//!   a batch is a prefix of the mutation sequence, a branch head can never
//!   become durable before the objects it names.
//! * **One batch at a time.** A flush that is already running is not joined by
//!   a second; it loops instead, so a later value of a key can never be
//!   overtaken by an earlier one.

use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use opendoc_app::VolumeRecoveryJournalStore;
use opendoc_store::{install_browser_volume, MirroredVolume};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    IdbDatabase, IdbFactory, IdbObjectStore, IdbRequest, IdbTransaction, IdbTransactionMode,
};

/// One database per origin, one object store in it: the volume is a flat
/// `key -> bytes` map and IndexedDB is being used as exactly that and nothing
/// more. No indexes, no per-document stores, no schema to migrate — the
/// meaning of a key is `opendoc-store`'s business, not the browser's.
const DATABASE: &str = "opendoc";
const OBJECT_STORE: &str = "volume";
const DATABASE_VERSION: u32 = 1;
/// The volume subtree the crash-recovery journal owns. Repositories live under
/// `repositories/`, so the two cannot collide.
const RECOVERY_PREFIX: &str = "recovery";

thread_local! {
    /// The volume exists from module start, before IndexedDB is reachable, so
    /// a command dispatched during boot still has somewhere to write.
    static VOLUME: RefCell<Option<MirroredVolume>> = const { RefCell::new(None) };
    static DATABASE_HANDLE: RefCell<Option<IdbDatabase>> = const { RefCell::new(None) };
    static FLUSHING: Cell<bool> = const { Cell::new(false) };
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    static PERSISTENT: Cell<bool> = const { Cell::new(false) };
}

/// Create the volume and install it as this runtime's local storage.
///
/// Called from the wasm-bindgen start function: it is synchronous, so the
/// volume is in place before any command can run. It is not yet durable —
/// [`storage_ready`] attaches IndexedDB to it.
pub(crate) fn install_volume() -> MirroredVolume {
    let volume = MirroredVolume::new();
    install_browser_volume(volume.clone());
    VOLUME.with(|slot| *slot.borrow_mut() = Some(volume.clone()));
    volume
}

pub(crate) fn volume() -> Option<MirroredVolume> {
    VOLUME.with(|slot| slot.borrow().clone())
}

/// A recovery-journal store over this runtime's volume, for the app to install.
pub(crate) fn recovery_journal_store() -> Option<Arc<VolumeRecoveryJournalStore>> {
    volume().map(|volume| Arc::new(VolumeRecoveryJournalStore::new(volume, RECOVERY_PREFIX)))
}

// ---- Awaiting IndexedDB ----------------------------------------------------

fn callback(handler: impl FnOnce(JsValue) + 'static) -> JsValue {
    Closure::once_into_js(move |event: JsValue| handler(event))
}

fn request_error(request: &IdbRequest) -> JsValue {
    match request.error() {
        Ok(Some(error)) => error.into(),
        _ => JsValue::from_str("IndexedDB request failed"),
    }
}

/// A promise that settles when `request` does, with the request's result.
fn request_promise(request: &IdbRequest) -> Promise {
    let request = request.clone();
    Promise::new(&mut |resolve, reject| {
        let success_request = request.clone();
        let on_success = callback(move |_| {
            let value = success_request.result().unwrap_or(JsValue::UNDEFINED);
            let _ = resolve.call1(&JsValue::NULL, &value);
        });
        let error_request = request.clone();
        let on_error = callback(move |_| {
            let _ = reject.call1(&JsValue::NULL, &request_error(&error_request));
        });
        request.set_onsuccess(Some(on_success.unchecked_ref::<Function>()));
        request.set_onerror(Some(on_error.unchecked_ref::<Function>()));
    })
}

/// A promise that settles when the whole transaction commits — the only signal
/// that says the batch is durable. Individual request successes do not.
fn transaction_promise(transaction: &IdbTransaction) -> Promise {
    let transaction = transaction.clone();
    Promise::new(&mut |resolve, reject| {
        let on_complete = callback(move |_| {
            let _ = resolve.call1(&JsValue::NULL, &JsValue::TRUE);
        });
        let reject_abort = reject.clone();
        let on_abort = callback(move |_| {
            let _ = reject_abort.call1(
                &JsValue::NULL,
                &JsValue::from_str("IndexedDB transaction aborted"),
            );
        });
        let on_error = callback(move |_| {
            let _ = reject.call1(
                &JsValue::NULL,
                &JsValue::from_str("IndexedDB transaction failed"),
            );
        });
        transaction.set_oncomplete(Some(on_complete.unchecked_ref::<Function>()));
        transaction.set_onabort(Some(on_abort.unchecked_ref::<Function>()));
        transaction.set_onerror(Some(on_error.unchecked_ref::<Function>()));
    })
}

/// `indexedDB` from whichever global this is (a window or a worker).
fn factory() -> Result<IdbFactory, JsValue> {
    let global = js_sys::global();
    let value = Reflect::get(&global, &JsValue::from_str("indexedDB"))?;
    if value.is_undefined() || value.is_null() {
        return Err(JsValue::from_str("this runtime has no IndexedDB"));
    }
    value
        .dyn_into::<IdbFactory>()
        .map_err(|_| JsValue::from_str("indexedDB is not an IDBFactory"))
}

async fn open_database() -> Result<IdbDatabase, JsValue> {
    let request = factory()?.open_with_u32(DATABASE, DATABASE_VERSION)?;
    let on_upgrade = callback(move |event: JsValue| {
        let Ok(target) = Reflect::get(&event, &JsValue::from_str("target")) else {
            return;
        };
        let request: IdbRequest = target.unchecked_into();
        let Ok(result) = request.result() else {
            return;
        };
        let database: IdbDatabase = result.unchecked_into();
        if !database.object_store_names().contains(OBJECT_STORE) {
            let _ = database.create_object_store(OBJECT_STORE);
        }
    });
    request.set_onupgradeneeded(Some(on_upgrade.unchecked_ref::<Function>()));
    let opened = JsFuture::from(request_promise(&request)).await?;
    opened
        .dyn_into::<IdbDatabase>()
        .map_err(|_| JsValue::from_str("IndexedDB open returned no database"))
}

fn readwrite_store(database: &IdbDatabase) -> Result<(IdbTransaction, IdbObjectStore), JsValue> {
    let transaction =
        database.transaction_with_str_and_mode(OBJECT_STORE, IdbTransactionMode::Readwrite)?;
    let store = transaction.object_store(OBJECT_STORE)?;
    Ok((transaction, store))
}

/// Everything the durable store holds, as `(key, bytes)`.
async fn read_all(database: &IdbDatabase) -> Result<Vec<(String, Vec<u8>)>, JsValue> {
    let transaction = database.transaction_with_str(OBJECT_STORE)?;
    let store = transaction.object_store(OBJECT_STORE)?;
    // `getAllKeys` and `getAll` both walk the store in key order, so the two
    // arrays line up index for index.
    let keys = JsFuture::from(request_promise(&store.get_all_keys()?)).await?;
    let values = JsFuture::from(request_promise(&store.get_all()?)).await?;
    let keys = Array::from(&keys);
    let values = Array::from(&values);
    let mut entries = Vec::with_capacity(keys.length() as usize);
    for index in 0..keys.length() {
        let Some(key) = keys.get(index).as_string() else {
            continue;
        };
        let value = values.get(index);
        if value.is_undefined() || value.is_null() {
            continue;
        }
        entries.push((key, Uint8Array::new(&value).to_vec()));
    }
    Ok(entries)
}

/// Write one batch of pending mutations in one transaction, then acknowledge it.
async fn flush_batch(database: &IdbDatabase, volume: &MirroredVolume) -> Result<bool, JsValue> {
    let pending = volume.pending();
    let Some(through) = pending.last().map(|mutation| mutation.seq) else {
        return Ok(false);
    };
    let (transaction, store) = readwrite_store(database)?;
    for mutation in &pending {
        let key = JsValue::from_str(&mutation.key);
        match &mutation.value {
            Some(bytes) => {
                let value = Uint8Array::from(bytes.as_slice());
                store.put_with_key(&value, &key)?;
            }
            None => {
                store.delete(&key)?;
            }
        }
    }
    JsFuture::from(transaction_promise(&transaction)).await?;
    // Only now, after the transaction committed, is the batch durable.
    volume.acknowledge(through);
    Ok(true)
}

/// Drain the volume into IndexedDB, without blocking the caller.
///
/// Called after every dispatched command. If a flush is already running it is
/// left to loop: that is what keeps batches serialized, so an older value of a
/// key can never land after a newer one.
pub(crate) fn schedule_flush() {
    let Some(volume) = volume() else {
        return;
    };
    let Some(database) = DATABASE_HANDLE.with(|slot| slot.borrow().clone()) else {
        return;
    };
    if volume.pending_len() == 0 || FLUSHING.with(Cell::get) {
        return;
    }
    FLUSHING.with(|flag| flag.set(true));
    spawn_local(async move {
        loop {
            match flush_batch(&database, &volume).await {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => {
                    // The batch stays pending, so the next command retries it.
                    // Reporting rather than panicking keeps the document open
                    // when storage is full or blocked.
                    let message = describe(&error);
                    web_sys::console::warn_1(&JsValue::from_str(&format!(
                        "opendoc: IndexedDB flush failed, work is still only in memory: {message}"
                    )));
                    LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(message));
                    break;
                }
            }
        }
        FLUSHING.with(|flag| flag.set(false));
    });
}

fn describe(error: &JsValue) -> String {
    error
        .as_string()
        .or_else(|| {
            Reflect::get(error, &JsValue::from_str("message"))
                .ok()
                .and_then(|value| value.as_string())
        })
        .unwrap_or_else(|| format!("{error:?}"))
}

fn stats(persistent: bool, entries: usize, recovery_sessions: usize) -> JsValue {
    let object = Object::new();
    let set = |key: &str, value: JsValue| {
        let _ = Reflect::set(&object, &JsValue::from_str(key), &value);
    };
    set("persistent", JsValue::from_bool(persistent));
    set("entries", JsValue::from_f64(entries as f64));
    set(
        "recoverySessions",
        JsValue::from_f64(recovery_sessions as f64),
    );
    if let Some(error) = LAST_ERROR.with(|slot| slot.borrow().clone()) {
        set("error", JsValue::from_str(&error));
    }
    object.into()
}

/// Attach durable storage, and report what was found.
///
/// The frontend awaits this once, before the first command: hydration has to
/// finish before the app can answer a read from the volume. Returns
/// `{ persistent, entries, recoverySessions }`. A runtime without IndexedDB
/// (jsdom, a hardened profile, private-mode failures) is not an error — the
/// volume keeps working in memory and `persistent` is false, which is exactly
/// the behaviour the browser build had before this existed.
pub(crate) async fn ready() -> Result<JsValue, JsValue> {
    let volume = volume().unwrap_or_else(install_volume);
    if PERSISTENT.with(Cell::get) {
        return Ok(stats(true, volume.len(), crate::recovery_session_count()));
    }

    let database = match open_database().await {
        Ok(database) => database,
        Err(error) => {
            let message = describe(&error);
            web_sys::console::warn_1(&JsValue::from_str(&format!(
                "opendoc: no durable browser storage, documents live only in this tab: {message}"
            )));
            LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(message));
            // Stop queueing mutations nothing will ever drain.
            volume.disable_mirroring();
            return Ok(stats(false, volume.len(), 0));
        }
    };

    let stored = read_all(&database).await?;
    // A command dispatched before hydration finished has already written to
    // the volume; a durable value must not overwrite a live one.
    let entries = stored
        .into_iter()
        .filter(|(key, _)| !volume.contains(key))
        .collect::<Vec<_>>();
    volume.hydrate(entries);

    DATABASE_HANDLE.with(|slot| *slot.borrow_mut() = Some(database));
    PERSISTENT.with(|flag| flag.set(true));

    let sessions = crate::install_recovery_journal();
    schedule_flush();
    Ok(stats(true, volume.len(), sessions))
}

/// Durability watermark, for tests and diagnostics: how far behind the durable
/// store is, and whether it is durable at all.
pub(crate) fn status() -> JsValue {
    let Some(volume) = volume() else {
        return stats(false, 0, 0);
    };
    let object = Object::new();
    let set = |key: &str, value: JsValue| {
        let _ = Reflect::set(&object, &JsValue::from_str(key), &value);
    };
    set("persistent", JsValue::from_bool(PERSISTENT.with(Cell::get)));
    set("entries", JsValue::from_f64(volume.len() as f64));
    set("sequence", JsValue::from_f64(volume.sequence() as f64));
    set("durableSeq", JsValue::from_f64(volume.durable_seq() as f64));
    set("pending", JsValue::from_f64(volume.pending_len() as f64));
    set(
        "pendingBytes",
        JsValue::from_f64(volume.pending_bytes() as f64),
    );
    set("flushing", JsValue::from_bool(FLUSHING.with(Cell::get)));
    if let Some(error) = LAST_ERROR.with(|slot| slot.borrow().clone()) {
        set("error", JsValue::from_str(&error));
    }
    object.into()
}
