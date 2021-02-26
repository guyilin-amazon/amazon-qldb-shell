use crate::repl_helper::ReplHelper;
use dirs;
use rustyline::error::ReadlineError;
use rustyline::{Config, Editor};
use std::cell::RefCell;
use std::{io, path::PathBuf};

struct UiInner {
    rl: Editor<ReplHelper>,
    prompt: String,
    pending_actions: Vec<String>,
}

/// Encapsulates handling of user input. In particular, we use readline to handle keyboard input (capturing lines, but also history and Emacs/Vi bindings) and support 'sending; multiple;
/// inputs'. We also capture history for uparrow or Ctrl-R replay.
///
/// This type users interior mutability because the [`QldbDriver::transact`] method takes a `Fn` arg (not `FnMut`) so that retries don't have side-effects. However, we disable retries and
/// thus don't care about the side-effects (e.g. of saving history).
pub(crate) struct Ui {
    inner: RefCell<UiInner>,
}

impl Ui {
    pub(crate) fn new() -> Ui {
        let config = Config::builder() // FIXME: customize :)
            .build();
        let mut rl = Editor::with_config(config);
        rl.set_helper(Some(ReplHelper::default()));

        if let Some(p) = history_path() {
            rl.load_history(&p).keep_going();
        }

        Ui {
            inner: RefCell::new(UiInner {
                rl,
                prompt: "> ".to_owned(),
                pending_actions: vec![],
            }),
        }
    }

    // This is a big hack. Some open questions:
    //
    // 1. How to support single statement transactions
    // 2. Really don't need all the readline stuff here
    // 3. Also don't want to load/persist history
    // 4. exit is awful
    pub(crate) fn new_for_script(script: &str) -> io::Result<Ui> {
        let config = Config::builder().build();
        let mut rl = Editor::with_config(config);
        rl.set_helper(Some(ReplHelper::default()));

        // We start the pending actions by reading the input, splitting it up into new lines..
        let mut pending_actions: Vec<_> = script
            .lines()
            .map(|line| line.split(";").map(|it| it.trim().to_owned()))
            .flatten()
            .collect();
        // ..and then adding an exit comment
        pending_actions.push("exit".to_string()); // totally not a hack.
        pending_actions.reverse(); // also not a hack

        Ok(Ui {
            inner: RefCell::new(UiInner {
                rl,
                prompt: "".to_owned(),
                pending_actions,
            }),
        })
    }

    pub(crate) fn set_prompt(&self, prompt: String) {
        self.inner.borrow_mut().prompt = prompt;
    }

    /// Prompts the user for input or returns the next pending action.
    ///
    /// Users can enter multiple commands like 'foo; bar'. These commands will be processed as if first 'foo' was entered and then 'bar', except that errors MUST halt the chain (see
    /// [`clear_pending`]).
    ///
    /// Note that the history will contain the actual input ('foo; bar' not 'foo' & 'bar'). Similarly, we trim the strings such that 'foo;bar' and 'foo; bar' are treated identically (but the
    /// history will have the raw input).
    pub(crate) fn user_input(&self) -> Result<String, ReadlineError> {
        let mut inner = self.inner.borrow_mut();

        if !inner.pending_actions.is_empty() {
            return Ok(inner.pending_actions.pop().unwrap());
        }

        let prompt = inner.prompt.clone();
        match inner.rl.readline(&prompt) {
            Ok(line) => {
                inner.rl.add_history_entry(line.as_str());
                inner.pending_actions = line.split(";").map(|it| it.trim().to_owned()).collect();
                inner.pending_actions.reverse();
                drop(inner);
                self.user_input()
            }
            err => err,
        }
    }

    /// Clear the queue of pending actions. This method should be called on error.
    pub(crate) fn clear_pending(&self) {
        self.inner.borrow_mut().pending_actions.clear();
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        if let Some(p) = history_path() {
            self.inner.borrow_mut().rl.save_history(&p).keep_going();
        }
    }
}

fn history_path() -> Option<PathBuf> {
    match dirs::home_dir() {
        Some(dir) => Some(dir.join(".qldbshell_history")),
        None => None,
    }
}

trait KeepGoing {
    fn keep_going(self) -> ();
}

impl KeepGoing for Result<(), ReadlineError> {
    fn keep_going(self) -> () {
        match self {
            Ok(_) => (),
            Err(e) => warn!("{}", e),
        }
    }
}
