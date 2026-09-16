//! The page's view of the shell: exactly two functions, and nothing else.
//!
//! # Why this exists
//!
//! `withGlobalTauri` publishes the whole `@tauri-apps/api` bundle on
//! `window.__TAURI__` — core, event, path, window, webview, app, menu, tray,
//! image. Every one of those is reachable from any script that runs in the
//! page, including one that got there through a rendering bug, and each is a
//! separate thing to reason about when asking what an injected script can do.
//!
//! The page does not use that surface. It uses `invoke` (through
//! `src/invoke.ts`) and `listen` (for `opendoc://collab-status` and
//! `opendoc://close-requested`). So `withGlobalTauri` is off in
//! `tauri.conf.json` and this script puts those two functions back, in the two
//! places `invoke.ts` already looks for them.
//!
//! This is a *reduction*, not a sandbox: `window.__TAURI_INTERNALS__.invoke` is
//! injected by Tauri itself and stays reachable whatever this script does. What
//! actually bounds a command is the capability file plus, for the file
//! commands, the grants in [`crate::fileaccess`]. This narrows the surface and
//! makes it explicit; it does not replace either of those.
//!
//! # Why an initialisation script
//!
//! It has to run before the page's own modules do. An initialisation script is
//! injected into the webview before any document script runs, and — on
//! WebKitGTK, through `UserContentManager` — is not itself subject to the
//! page's content-security policy, which is why a strict `script-src 'self'`
//! does not block it.
//!
//! When `invoke.ts` moves to `__TAURI_INTERNALS__` for events as well as for
//! `invoke`, this file can be deleted outright.

/// The bridge, as JavaScript.
///
/// `listen` mirrors `@tauri-apps/api`'s: the handler is registered through
/// `transformCallback`, the plugin command returns an id, and the returned
/// function unlistens with it and drops the callback. `unregisterListener`
/// comes from the event plugin's own initialisation script, which Tauri
/// injects whether or not the global API bundle is enabled.
const BRIDGE: &str = r#"
(function () {
  var internals = window.__TAURI_INTERNALS__;
  if (!internals || typeof internals.invoke !== "function") {
    return;
  }
  function invoke(command, args, options) {
    return internals.invoke(command, args, options);
  }
  async function listen(event, handler, options) {
    var target =
      typeof (options && options.target) === "string"
        ? { kind: "AnyLabel", label: options.target }
        : (options && options.target) || { kind: "Any" };
    var id = await invoke("plugin:event|listen", {
      event: event,
      target: target,
      handler: internals.transformCallback(handler),
    });
    return async function () {
      var plugin = window.__TAURI_EVENT_PLUGIN_INTERNALS__;
      if (plugin && typeof plugin.unregisterListener === "function") {
        plugin.unregisterListener(event, id);
      }
      await invoke("plugin:event|unlisten", { event: event, eventId: id });
    };
  }
  Object.defineProperty(window, "__TAURI__", {
    value: Object.freeze({
      core: Object.freeze({ invoke: invoke }),
      event: Object.freeze({ listen: listen }),
    }),
    configurable: false,
    enumerable: false,
    writable: false,
  });
})();
"#;

/// A plugin that contributes nothing but the initialisation script above.
pub(crate) fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("opendoc-bridge")
        .js_init_script(BRIDGE)
        .build()
}

#[cfg(test)]
mod tests {
    use super::BRIDGE;

    /// The two names `apps/desktop/src/invoke.ts` reads, and nothing broader.
    ///
    /// If the frontend starts using another part of the Tauri API this test is
    /// where the omission shows up, rather than as a feature that silently
    /// stops working in the packaged app.
    #[test]
    fn the_bridge_publishes_exactly_invoke_and_listen() {
        assert!(BRIDGE.contains("core: Object.freeze({ invoke: invoke })"));
        assert!(BRIDGE.contains("event: Object.freeze({ listen: listen })"));
        for absent in ["path", "webview", "menu", "tray", "app:", "emit"] {
            assert!(!BRIDGE.contains(absent), "the bridge mentions {absent}");
        }
    }

    /// It has to survive a page that has no Tauri at all (the browser build
    /// loads the same modules) rather than throwing during startup.
    #[test]
    fn the_bridge_does_nothing_without_the_internals_it_wraps() {
        assert!(BRIDGE.contains("if (!internals || typeof internals.invoke !== \"function\")"));
    }
}
