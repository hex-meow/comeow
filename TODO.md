# TODO

## UTF-8 / wide-character (CJK) last-character rendering glitch

**Symptom (reported):** when typing UTF-8 (e.g. CJK) characters at the
interactive prompt, the *last* character sometimes isn't shown until another
key is pressed (then deleted).

**Status:** not reproduced from captured output; mechanism identified; a
workaround flag (`--no-highlight`) is in place. Needs confirmation in a real
terminal.

**Findings so far:**
- Our live highlighter makes `Highlighter::highlight_char` return `true`, so
  rustyline does a **full line refresh on every keystroke** instead of its
  optimised single-character insert path (`edit.rs::edit_insert`). That
  optimised path is the one that natively handles wide (2-column) characters.
- A PTY capture (`/tmp/ptytest*.py`) at 80 columns shows rustyline *does* emit
  every CJK byte and repositions the cursor to the correct column (incl. across
  the line-wrap boundary). So in rustyline's own model the bytes are correct —
  the dropped glyph appears to be a terminal-level rendering interaction with
  the full-refresh path, which a byte capture can't surface.
- `rustyline::tty::unix::calculate_position` + `width()` handle our SGR escapes
  as zero-width correctly, so colour codes are not miscounted.

**Next steps to try:**
1. Confirm cause in the user's real terminal: does `comeow --no-highlight`
   make the glitch disappear? If yes, the full-refresh highlighting path is the
   culprit.
2. If confirmed, options:
   - keep `--no-highlight` as the escape hatch (done), or
   - investigate a rustyline config / upstream fix for wide chars during
     full refresh, or
   - file an issue upstream (rustyline 14) with a minimal repro.
3. Try other terminals/emulators and a non-default `COLUMNS` to see if it's
   wrap-boundary specific.
