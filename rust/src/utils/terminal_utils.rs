// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::io;

use anyhow::Result;

/// Clears the terminal and move the cursor to the top.
pub fn clear() {
    println!("\x1B[2J\x1B[1;1H");
}

/// Blocks the terminal until user presses enter.
pub fn wait_enter(wait_keys: &str) -> Result<()> {
    let mut stdin = String::new();
    io::stdin().read_line(&mut stdin)?;
    if wait_keys.is_empty() || stdin.contains(wait_keys) {
        Ok(())
    } else {
        wait_enter(wait_keys)
    }
}

#[cfg(test)]
pub mod mock {
    use anyhow::Result;

    pub fn clear() {
        return;
    }
    pub fn wait_enter(_wait_keys: &str) -> Result<()> {
        Ok(())
    }
}
