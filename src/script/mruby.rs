//! Safe wrapper around the C shim in `mruby/shim.c`.

use std::ffi::{c_char, c_int, CStr, CString};
use std::fmt;

#[repr(C)]
struct MrbState {
    _private: [u8; 0],
}

extern "C" {
    fn knut_mrb_open() -> *mut MrbState;
    fn knut_mrb_close(mrb: *mut MrbState);
    fn knut_mrb_free(text: *mut c_char);
    fn knut_mrb_eval(
        mrb: *mut MrbState,
        code: *const c_char,
        len: usize,
        filename: *const c_char,
        out: *mut *mut c_char,
    ) -> c_int;
}

/// An exception raised while evaluating Ruby code: the exception's
/// `inspect` output followed by its backtrace.
#[derive(Debug, Clone)]
pub(crate) struct RubyError {
    pub message: String,
    pub backtrace: Vec<String>,
}

impl fmt::Display for RubyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        for line in &self.backtrace {
            write!(f, "\n    from {line}")?;
        }
        Ok(())
    }
}

impl std::error::Error for RubyError {}

/// One mruby interpreter. mruby is single-threaded, so this is neither
/// `Send` nor `Sync`.
pub(crate) struct Mruby {
    state: *mut MrbState,
}

impl Mruby {
    pub(crate) fn new() -> Self {
        let state = unsafe { knut_mrb_open() };
        assert!(!state.is_null(), "mruby failed to allocate its state");
        Self { state }
    }

    /// Evaluates `code` and returns the result's `to_s`. `filename` is used
    /// in error messages and backtraces.
    pub(crate) fn eval(&mut self, code: &str, filename: &str) -> Result<String, RubyError> {
        let filename = CString::new(filename).unwrap_or_else(|_| c"(invalid)".into());
        let mut out: *mut c_char = std::ptr::null_mut();
        let failed = unsafe {
            knut_mrb_eval(
                self.state,
                code.as_ptr().cast(),
                code.len(),
                filename.as_ptr(),
                &mut out,
            )
        };
        let text = if out.is_null() {
            String::from("out of memory")
        } else {
            let text = unsafe { CStr::from_ptr(out) }
                .to_string_lossy()
                .into_owned();
            unsafe { knut_mrb_free(out) };
            text
        };
        if failed == 0 {
            return Ok(text);
        }
        let mut lines = text.lines().map(str::to_owned);
        Err(RubyError {
            message: lines.next().unwrap_or_default(),
            backtrace: lines.collect(),
        })
    }
}

impl Default for Mruby {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Mruby {
    fn drop(&mut self) {
        unsafe { knut_mrb_close(self.state) };
    }
}

/// Writes `text` as a double-quoted Ruby string literal.
pub(crate) fn ruby_string(text: &str) -> String {
    let mut literal = String::with_capacity(text.len() + 2);
    literal.push('"');
    for ch in text.chars() {
        match ch {
            '"' => literal.push_str("\\\""),
            '\\' => literal.push_str("\\\\"),
            '#' => literal.push_str("\\#"),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\t' => literal.push_str("\\t"),
            ch if ch.is_control() => literal.push_str(&format!("\\u{{{:x}}}", ch as u32)),
            ch => literal.push(ch),
        }
    }
    literal.push('"');
    literal
}

/// Writes a JSON value as an equivalent Ruby literal (objects become hashes
/// with string keys).
pub(crate) fn ruby_literal(value: &serde_json::Value) -> String {
    use serde_json::Value;
    match value {
        Value::Null => "nil".into(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => ruby_string(text),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(ruby_literal).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(fields) => {
            let fields: Vec<String> = fields
                .iter()
                .map(|(key, value)| format!("{} => {}", ruby_string(key), ruby_literal(value)))
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_and_reports_errors_with_location() {
        let mut ruby = Mruby::new();
        assert_eq!(ruby.eval("1 + 2", "test.rb").unwrap(), "3");

        let error = ruby.eval("\n\nraise 'boom'", "test.rb").unwrap_err();
        assert!(error.message.contains("boom"), "{error}");
        assert!(error.to_string().contains("test.rb:3"), "{error}");
    }

    #[test]
    fn reports_syntax_errors() {
        let mut ruby = Mruby::new();
        let error = ruby.eval("def broken(", "test.rb").unwrap_err();
        assert!(error.to_string().contains("test.rb"), "{error}");
    }

    #[test]
    fn string_literals_round_trip() {
        let mut ruby = Mruby::new();
        let text = "Küche \"#{evil}\" \\ \n end";
        assert_eq!(ruby.eval(&ruby_string(text), "test.rb").unwrap(), text);
    }
}
