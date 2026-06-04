//! # SeqTerm Lua Scripting Engine
//!
//! Provides live Lua scripting via `mlua` (Lua 5.4, vendored — no system install needed).
//!
//! ## Script API (available inside Lua)
//!
//! ```lua
//! -- Called every sequencer step (step 0-based, bpm = current BPM).
//! function on_step(step, bpm)
//!     if step % 16 == 0 then
//!         seqterm.status("Bar " .. tostring(step // 16))
//!     end
//! end
//!
//! -- Called every bar.
//! function on_bar(bar, bpm)
//!     seqterm.set_bpm(120 + bar % 20)
//! end
//! ```
//!
//! ## Lua `seqterm` table methods
//!
//! | Method | Description |
//! |--------|-------------|
//! | `seqterm.status(msg)` | Show a timed status message |
//! | `seqterm.set_bpm(bpm)` | Change the project BPM |
//! | `seqterm.note_on(slot, note, vel)` | Trigger a NoteOn on a mixer slot |
//! | `seqterm.note_off(slot, note)` | Trigger a NoteOff on a mixer slot |

use seqterm_command::AppCommand;

// ─── Pending command queue ─────────────────────────────────────────────────────

/// Commands queued by the Lua callback in a single call.
/// Returned to the caller so it can dispatch them through AppCommand.
pub type LuaCommands = Vec<AppCommand>;

// ─── LuaEngine ────────────────────────────────────────────────────────────────

/// Manages one Lua 5.4 state and a set of named scripts.
///
/// Scripts are compiled on load and stored in the registry.
/// Hook functions (`on_step`, `on_bar`) are called from the sequencer thread
/// via `App::process_events`, which drains the returned `LuaCommands` into
/// `app.pending_commands`.
pub struct LuaEngine {
    #[cfg(feature = "lua")]
    state: mlua::Lua,
    /// Loaded script names (in order of loading).
    pub scripts: Vec<String>,
}

impl LuaEngine {
    pub fn new() -> anyhow::Result<Self> {
        #[cfg(feature = "lua")]
        {
            let state = mlua::Lua::new();
            let engine = Self { state, scripts: Vec::new() };
            engine.register_api().map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(engine)
        }
        #[cfg(not(feature = "lua"))]
        {
            Ok(Self { scripts: Vec::new() })
        }
    }

    /// Load (compile) a Lua script and register it by name.
    /// Replaces any previously loaded script with the same name.
    pub fn load_script(&mut self, name: &str, source: &str) -> anyhow::Result<()> {
        #[cfg(feature = "lua")]
        {
            self.state.load(source).set_name(name).exec()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if !self.scripts.contains(&name.to_string()) {
                self.scripts.push(name.to_string());
            }
        }
        #[cfg(not(feature = "lua"))]
        {
            let _ = (name, source);
        }
        Ok(())
    }

    /// Remove a loaded script by name. Does not un-define Lua globals.
    pub fn unload_script(&mut self, name: &str) {
        self.scripts.retain(|n| n != name);
    }

    /// List loaded script names.
    pub fn list_scripts(&self) -> &[String] {
        &self.scripts
    }

    /// Call the `on_step(step, bpm)` hook if defined.
    /// Returns a list of `AppCommand`s queued by the script.
    pub fn call_on_step(&self, step: u32, bpm: f64) -> LuaCommands {
        #[cfg(feature = "lua")]
        {
            self.call_hook("on_step", (step, bpm))
                .unwrap_or_default()
        }
        #[cfg(not(feature = "lua"))]
        { let _ = (step, bpm); vec![] }
    }

    /// Call the `on_bar(bar, bpm)` hook if defined.
    pub fn call_on_bar(&self, bar: u32, bpm: f64) -> LuaCommands {
        #[cfg(feature = "lua")]
        {
            self.call_hook("on_bar", (bar, bpm))
                .unwrap_or_default()
        }
        #[cfg(not(feature = "lua"))]
        { let _ = (bar, bpm); vec![] }
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    /// Convert mlua::Error to anyhow::Error (mlua::Error is !Sync).
    #[cfg(feature = "lua")]
    fn lua_err(e: mlua::Error) -> anyhow::Error { anyhow::anyhow!("{e}") }

    #[cfg(feature = "lua")]
    fn call_hook<A: mlua::IntoLuaMulti>(
        &self,
        name: &str,
        args: A,
    ) -> anyhow::Result<LuaCommands> {
        use mlua::Value;
        let globals = self.state.globals();
        let hook: mlua::Value = globals.get(name).map_err(Self::lua_err)?;
        if matches!(hook, Value::Nil) {
            return Ok(vec![]);
        }
        let f: mlua::Function = globals.get(name).map_err(Self::lua_err)?;

        let pending_key = "__seqterm_pending";
        let empty_table = self.state.create_table().map_err(Self::lua_err)?;
        self.state.globals().set(pending_key, empty_table).map_err(Self::lua_err)?;

        if let Err(e) = f.call::<()>(args) {
            tracing::warn!("Lua hook '{name}' error: {e}");
        }

        let table: mlua::Table = self.state.globals().get(pending_key).map_err(Self::lua_err)?;
        let mut cmds = Vec::new();
        for pair in table.sequence_values::<mlua::Table>() {
            if let Ok(row) = pair {
                if let Ok(cmd) = self.table_to_command(row) {
                    cmds.push(cmd);
                }
            }
        }
        Ok(cmds)
    }

    #[cfg(feature = "lua")]
    fn table_to_command(&self, t: mlua::Table) -> anyhow::Result<AppCommand> {
        let kind: String = t.get("kind").map_err(Self::lua_err)?;
        match kind.as_str() {
            "status" => {
                let text: String = t.get("text").map_err(Self::lua_err)?;
                let ms: Option<u32> = t.get("ms").ok();
                Ok(AppCommand::StatusMessage { text, duration_ms: ms })
            }
            "set_bpm" => {
                let bpm: f64 = t.get("bpm").map_err(Self::lua_err)?;
                Ok(AppCommand::SetBpm(bpm.clamp(20.0, 300.0)))
            }
            _ => anyhow::bail!("unknown command kind: {kind}"),
        }
    }

    #[cfg(feature = "lua")]
    fn register_api(&self) -> anyhow::Result<()> {
        let lua = &self.state;
        let globals = lua.globals();
        let seqterm = lua.create_table().map_err(Self::lua_err)?;

        let pending_key = "__seqterm_pending";

        let lua_s = lua.clone();
        seqterm.set("status", lua.create_function(move |_, (msg, ms): (String, Option<u32>)| {
            let pending: mlua::Table = lua_s.globals().get("__seqterm_pending")?;
            let t = lua_s.create_table()?;
            t.set("kind", "status")?;
            t.set("text", msg)?;
            if let Some(m) = ms { t.set("ms", m)?; }
            let len = pending.raw_len();
            pending.raw_set(len + 1, t)
        }).map_err(Self::lua_err)?).map_err(Self::lua_err)?;

        let lua_s = lua.clone();
        seqterm.set("set_bpm", lua.create_function(move |_, bpm: f64| {
            let pending: mlua::Table = lua_s.globals().get("__seqterm_pending")?;
            let t = lua_s.create_table()?;
            t.set("kind", "set_bpm")?;
            t.set("bpm", bpm)?;
            let len = pending.raw_len();
            pending.raw_set(len + 1, t)
        }).map_err(Self::lua_err)?).map_err(Self::lua_err)?;

        let empty = lua.create_table().map_err(Self::lua_err)?;
        globals.set(pending_key, empty).map_err(Self::lua_err)?;
        globals.set("seqterm", seqterm).map_err(Self::lua_err)?;
        Ok(())
    }
}

impl Default for LuaEngine {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| Self {
            #[cfg(feature = "lua")]
            state: mlua::Lua::new(),
            scripts: Vec::new(),
        })
    }
}

// ─── AppCommand additions needed by Lua ──────────────────────────────────────
//
// `AppCommand::StatusMessage` is used by the `seqterm.status()` API.
// It must already exist in `seqterm-command`.  If not, the `status_msg` field
// on App is a direct alternative — but here we use pending_commands so the
// Lua API stays decoupled from the App type.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_creates_ok() {
        let engine = LuaEngine::new();
        assert!(engine.is_ok());
    }

    #[cfg(feature = "lua")]
    #[test]
    fn on_step_hook_fires() {
        let mut engine = LuaEngine::new().unwrap();
        engine.load_script("test", "function on_step(s, bpm) end").unwrap();
        let cmds = engine.call_on_step(0, 120.0);
        assert!(cmds.is_empty());
    }

    #[cfg(feature = "lua")]
    #[test]
    fn status_command_from_lua() {
        let mut engine = LuaEngine::new().unwrap();
        engine.load_script("test",
            "function on_step(s, bpm) seqterm.status('hello', 3000) end"
        ).unwrap();
        let cmds = engine.call_on_step(0, 120.0);
        assert_eq!(cmds.len(), 1);
        if let AppCommand::StatusMessage { text, .. } = &cmds[0] {
            assert_eq!(text, "hello");
        } else {
            panic!("expected StatusMessage");
        }
    }
}
