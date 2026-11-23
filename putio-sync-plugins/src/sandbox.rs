//! A fresh Lua state for one plugin (research R2): only the string, table, math and utf8
//! libraries, no way to load files or binary chunks, a memory limit for
//! the whole state, and an instruction hook that stops the script past its deadline.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use mlua::{HookTriggers, Lua, LuaOptions, StdLib, VmState};

/// The text of the error the instruction hook raises past the deadline
pub const TIME_LIMIT: &str = "time limit reached";

/// When the current call must end; `None` between calls
pub type Deadline = Rc<Cell<Option<Instant>>>;

// `expired` is passed in. pcall and xpcall re-raise any error once the deadline has
// passed, so a script cannot catch the time limit and carry on. There is no coroutine
// library: a coroutine does not inherit the instruction hook, so a loop in one would
// never stop.
//
// The hook only fires between VM instructions, so one long C call (`string.rep`, a pattern
// that backtracks) would run past the deadline. The string functions that can do that are
// wrapped: `rep` caps its result and the pattern functions cap the subject and pattern
// length and how many quantifiers a pattern has. Plain `string.find` is linear and only
// has the subject cap lifted.
const SETUP: &str = r#"
local expired = ...
local raw_pcall, raw_xpcall, raw_error = pcall, xpcall, error
local function check(ok, ...)
  if not ok and expired() then raw_error((...), 0) end
  return ok, ...
end
pcall = function(...) return check(raw_pcall(...)) end
xpcall = function(...) return check(raw_xpcall(...)) end
dofile = nil
loadfile = nil
collectgarbage = nil
print = nil
string.dump = nil
local MAX_REP, MAX_SUBJECT, MAX_PATTERN = 4194304, 1048576, 512
local MAX_QUANT, MAX_LOOSE = 16, 8
local raw_rep, raw_find, raw_match = string.rep, string.find, string.match
local raw_gmatch, raw_gsub, raw_tostring, raw_type = string.gmatch, string.gsub, tostring, type
local function text_len(v)
  local t = raw_type(v)
  if t == "string" then return #v end
  if t == "number" then return #raw_tostring(v) end
  return 0
end
local function guard_subject(s)
  if text_len(s) > MAX_SUBJECT then raw_error("string too long to match", 3) end
end
local function guard_pattern(p)
  if raw_type(p) ~= "string" then return end
  if #p > MAX_PATTERN then raw_error("pattern too long", 3) end
  local _, loose = raw_gsub(p, "[%*%+%-]", "")
  local _, quant = raw_gsub(p, "[%*%+%-%?]", "")
  if loose > MAX_LOOSE or quant > MAX_QUANT then raw_error("pattern too complex", 3) end
end
string.rep = function(s, n, sep)
  if raw_type(n) == "number" and n > 1 then
    if (text_len(s) + text_len(sep)) * n > MAX_REP then
      raw_error("resulting string too large", 2)
    end
  end
  return raw_rep(s, n, sep)
end
string.find = function(s, p, init, plain)
  guard_subject(s)
  if not plain then guard_pattern(p) end
  return raw_find(s, p, init, plain)
end
string.match = function(s, p, init)
  guard_subject(s) guard_pattern(p)
  return raw_match(s, p, init)
end
string.gmatch = function(s, p, init)
  guard_subject(s) guard_pattern(p)
  return raw_gmatch(s, p, init)
end
string.gsub = function(s, p, repl, n)
  guard_subject(s) guard_pattern(p)
  return raw_gsub(s, p, repl, n)
end
local raw_load = load
load = function(chunk, name, _mode, env)
  return raw_load(chunk, name, "t", env)
end
"#;

/// A new sandboxed state with `memory_mb` MiB and the deadline hook installed
pub fn new_state(memory_mb: u64, deadline: Deadline) -> mlua::Result<Lua> {
    let libs = StdLib::STRING | StdLib::TABLE | StdLib::MATH | StdLib::UTF8;
    let lua = Lua::new_with(libs, LuaOptions::default())?;
    let past = deadline.clone();
    let expired =
        lua.create_function(move |_, ()| Ok(past.get().is_some_and(|d| Instant::now() >= d)))?;
    lua.load(SETUP).set_name("=sandbox").call::<()>(expired)?;
    lua.set_memory_limit((memory_mb as usize) * 1024 * 1024)?;
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(1000),
        move |_, _| match deadline.get() {
            Some(d) if Instant::now() >= d => Err(mlua::Error::runtime(TIME_LIMIT)),
            _ => Ok(VmState::Continue),
        },
    )?;
    Ok(lua)
}

/// The first line of an error, without Rust's wrapping, at most 500 characters
pub fn error_text(e: &mlua::Error) -> String {
    let text = match e {
        mlua::Error::RuntimeError(m) => m.clone(),
        mlua::Error::CallbackError { cause, .. } => return error_text(cause),
        mlua::Error::MemoryError(_) => "out of memory".to_string(),
        mlua::Error::SyntaxError { message, .. } => message.clone(),
        mlua::Error::WithContext { cause, .. } => return error_text(cause),
        other => other.to_string(),
    };
    let line = text.lines().next().unwrap_or("").trim();
    let mut out: String = line.chars().take(500).collect();
    if out.is_empty() {
        out = "error".to_string();
    }
    out
}

/// Whether the error came from the memory limit, possibly through a callback
pub fn is_memory_error(e: &mlua::Error) -> bool {
    match e {
        mlua::Error::MemoryError(_) => true,
        mlua::Error::CallbackError { cause, .. } | mlua::Error::WithContext { cause, .. } => {
            is_memory_error(cause)
        }
        other => other.to_string().contains("not enough memory"),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn state() -> (Lua, Deadline) {
        let deadline: Deadline = Rc::new(Cell::new(None));
        (new_state(8, deadline.clone()).unwrap(), deadline)
    }

    #[test]
    fn forbidden_globals_are_nil() {
        let (lua, _) = state();
        for name in [
            "os",
            "io",
            "debug",
            "package",
            "require",
            "dofile",
            "loadfile",
            "collectgarbage",
            "print",
            "coroutine",
        ] {
            let v: mlua::Value = lua.globals().get(name).unwrap();
            assert!(v.is_nil(), "{} is {:?}", name, v);
        }
        let dump: mlua::Value = lua.load("return string.dump").eval().unwrap();
        assert!(dump.is_nil());
        let ok: i64 = lua
            .load("return string.len('abc') + math.floor(1.5) + #table.pack(1, 2) + utf8.len('é')")
            .eval()
            .unwrap();
        assert_eq!(ok, 7);
    }

    #[test]
    fn long_string_calls_are_capped() {
        let (lua, _) = state();
        let started = Instant::now();
        let e = lua.load("return string.rep('x', 1e9)").exec().unwrap_err();
        assert!(error_text(&e).contains("too large"), "{}", e);
        let e = lua.load("return ('ab'):rep(3e6, ',')").exec().unwrap_err();
        assert!(error_text(&e).contains("too large"), "{}", e);
        let e = lua
            .load("return ('a'):find(('a?'):rep(30) .. ('a'):rep(30))")
            .exec()
            .unwrap_err();
        assert!(error_text(&e).contains("too complex"), "{}", e);
        let e = lua
            .load("return (('x'):rep(2e6)):gsub('x', 'y')")
            .exec()
            .unwrap_err();
        assert!(error_text(&e).contains("too long"), "{}", e);
        let e = lua
            .load("return ('x'):match(('x'):rep(600))")
            .exec()
            .unwrap_err();
        assert!(error_text(&e).contains("pattern too long"), "{}", e);
        let e = lua
            .load("for w in ('x'):gmatch('.-.-.-.-.-.-.-.-.-x') do end")
            .exec()
            .unwrap_err();
        assert!(error_text(&e).contains("too complex"), "{}", e);
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn normal_string_use_still_works() {
        let (lua, _) = state();
        let ok: bool = lua
            .load(
                r#"
                assert(string.rep('ab', 3, ',') == 'ab,ab,ab' and ('x'):rep(0) == '')
                assert(('/a//'):gsub('/+$', '') == '/a')
                assert(('http://host:80/p'):match('^%a+://([^/:?#]+)') == 'host')
                assert(('a.b_c'):gsub('[._]', ' ') == 'a b c')
                local show, ep = ('Show Name S01E02'):match('^(.-)%s*([Ss]%d%d?[Ee]%d%d%d?)')
                assert(show == 'Show Name' and ep == 'S01E02')
                assert(('Movie (2019) x'):match('^(.-)%s+[%(%[]?([12][09]%d%d)[%)%]]?%f[%W]'))
                assert(('x'):rep(1e6):find('y', 1, true) == nil)
                local n = 0 for _ in ('a,b,c'):gmatch('[^,]+') do n = n + 1 end
                assert(n == 3)
                return true"#,
            )
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn load_takes_text_only() {
        let (lua, _) = state();
        let n: i64 = lua.load("return load('return 41 + 1')()").eval().unwrap();
        assert_eq!(n, 42);
        let (f, err): (mlua::Value, String) = lua
            .load("return load('\\27Lua\\84\\0', 'x', 'b')")
            .eval()
            .unwrap();
        assert!(f.is_nil());
        assert!(err.contains("binary"), "{}", err);
    }

    #[test]
    fn a_loop_stops_at_the_deadline() {
        let (lua, deadline) = state();
        deadline.set(Some(Instant::now() + Duration::from_millis(200)));
        let started = Instant::now();
        let e = lua.load("while true do end").exec().unwrap_err();
        assert!(error_text(&e).contains(TIME_LIMIT), "{}", e);
        assert!(started.elapsed() < Duration::from_millis(700));
        // pcall cannot hold it off: the hook fires again outside.
        let e = lua
            .load("while true do pcall(function() while true do end end) end")
            .exec()
            .unwrap_err();
        assert!(error_text(&e).contains(TIME_LIMIT));
        deadline.set(None);
        let (ok, msg): (bool, String) = lua.load("return pcall(error, 'mine', 0)").eval().unwrap();
        assert!(!ok && msg == "mine");
        lua.load("for i = 1, 1e6 do end").exec().unwrap();
    }

    #[test]
    fn allocation_past_the_limit_is_a_memory_error() {
        let (lua, _) = state();
        let e = lua
            .load("local t = {} for i = 1, 1e8 do t[i] = string.rep('x', 64) .. i end")
            .exec()
            .unwrap_err();
        assert!(is_memory_error(&e), "{}", e);
        assert_eq!(error_text(&e), "out of memory");
    }

    #[test]
    fn errors_are_one_line() {
        let (lua, _) = state();
        let e = lua
            .load("error('boom\\nsecond line')")
            .set_name("@main.lua")
            .exec()
            .unwrap_err();
        assert_eq!(error_text(&e), "main.lua:1: boom");
    }
}
