// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::io::{self, Read};

use anyhow::Result;

/// Clears the terminal and move the cursor to the top.
pub fn clear() {
    println!("\x1B[2J\x1B[1;1H");
}

/// Blocks the terminal until user presses enter.
pub fn wait_enter() -> Result<()> {
    let mut stdin = io::stdin();
    stdin.read(&mut [0u8])?;
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
}
