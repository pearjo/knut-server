/*
 * The whole surface between Knut and mruby: open, evaluate a string, close.
 *
 * mrb_value is a boxed type whose layout depends on mruby's build options,
 * so it never crosses into Rust. Values travel as strings instead.
 */

#include <stdlib.h>
#include <string.h>

#include <mruby.h>
#include <mruby/array.h>
#include <mruby/compile.h>
#include <mruby/string.h>

mrb_state *knut_mrb_open(void) { return mrb_open(); }

void knut_mrb_close(mrb_state *mrb) { mrb_close(mrb); }

void knut_mrb_free(char *str) { free(str); }

static char *copy_string(mrb_state *mrb, mrb_value str) {
  mrb_int len = RSTRING_LEN(str);
  char *copy = malloc((size_t)len + 1);
  if (copy == NULL) {
    return NULL;
  }
  memcpy(copy, RSTRING_PTR(str), (size_t)len);
  copy[len] = '\0';
  return copy;
}

/*
 * Formats the pending exception as "<inspect>\n<backtrace lines>" and clears
 * it. Syntax errors only say "line N: ...", so `filename` is put in front.
 */
static char *take_exception(mrb_state *mrb, const char *filename) {
  mrb_value exc = mrb_obj_value(mrb->exc);
  /* Once mrb->exc is cleared, nothing else keeps the exception alive, and
     the calls below allocate and may run the garbage collector. */
  mrb_gc_protect(mrb, exc);
  mrb->exc = NULL;

  mrb_value text = mrb_funcall(mrb, exc, "inspect", 0);
  if (mrb->exc != NULL || !mrb_string_p(text)) {
    mrb->exc = NULL;
    text = mrb_str_new_cstr(mrb, "unprintable exception");
  }
  text = mrb_str_dup(mrb, text);
  if (mrb_obj_is_kind_of(mrb, exc, E_SYNTAX_ERROR) && RSTRING_LEN(text) > 5 &&
      memcmp(RSTRING_PTR(text), "line ", 5) == 0) {
    mrb_value located = mrb_str_new_cstr(mrb, filename);
    mrb_str_cat_lit(mrb, located, ":");
    mrb_str_cat(mrb, located, RSTRING_PTR(text) + 5, RSTRING_LEN(text) - 5);
    text = located;
  }

  mrb_value backtrace = mrb_funcall(mrb, exc, "backtrace", 0);
  if (mrb->exc == NULL && mrb_array_p(backtrace)) {
    for (mrb_int i = 0; i < RARRAY_LEN(backtrace); i++) {
      mrb_value line = mrb_ary_ref(mrb, backtrace, i);
      if (mrb_string_p(line)) {
        mrb_str_cat_lit(mrb, text, "\n");
        mrb_str_cat_str(mrb, text, line);
      }
    }
  }
  mrb->exc = NULL;
  return copy_string(mrb, text);
}

/*
 * Evaluates `code`, attributing it to `filename` in errors and backtraces.
 * Returns 0 on success and stores `result.to_s` in *out; returns 1 on an
 * exception and stores its description in *out. *out must be released with
 * knut_mrb_free.
 */
int knut_mrb_eval(mrb_state *mrb, const char *code, size_t len, const char *filename, char **out) {
  int arena = mrb_gc_arena_save(mrb);
  mrbc_context *ctx = mrbc_context_new(mrb);
  mrbc_filename(mrb, ctx, filename);
  ctx->capture_errors = TRUE;

  mrb_value result = mrb_load_nstring_cxt(mrb, code, len, ctx);
  mrbc_context_free(mrb, ctx);

  int failed = 0;
  if (mrb->exc != NULL) {
    *out = take_exception(mrb, filename);
    failed = 1;
  } else {
    mrb_value text = mrb_obj_as_string(mrb, result);
    if (mrb->exc != NULL) {
      *out = take_exception(mrb, filename);
      failed = 1;
    } else {
      *out = copy_string(mrb, text);
    }
  }

  mrb_gc_arena_restore(mrb, arena);
  return failed;
}
