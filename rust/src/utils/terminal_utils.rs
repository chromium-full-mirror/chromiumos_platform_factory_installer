// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::io;

use anyhow::Result;
use console;

/// Clears the terminal and move the cursor to the top.
pub fn clear() {
    println!("\x1B[2J\x1B[1;1H");
}

/// Blocks the terminal until user presses enter.
pub fn wait_enter() -> Result<()> {
    let mut stdin = String::new();
    io::stdin().read_line(&mut stdin)?;
    Ok(())
}

/// Blocks the terminal until user presses specific keys in sequence.
pub fn wait_keys(wait_keys: &str) -> Result<()> {
    let mut i = 0;
    let len = wait_keys.len();
    while i < len {
        let chr = console::Term::stdout().read_char()?;
        if chr == wait_keys.chars().nth(i).unwrap() {
            i += 1;
        } else {
            i = 0;
        }
    }
    Ok(())
}

#[cfg(test)]
pub mod mock {
    use anyhow::Result;

    pub fn clear() {
        return;
    }
    pub fn wait_enter() -> Result<()> {
        Ok(())
    }

    pub fn wait_keys(_wait_keys: &str) -> Result<()> {
        Ok(())
    }
}
