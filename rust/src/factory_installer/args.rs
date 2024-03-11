// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fmt;

pub use clap::{Parser, Subcommand};

#[derive(Debug, Subcommand)]
pub enum Action {
    FAI {
        /// Output the collected data to the file path
        #[clap(short, long)]
        output_path: Option<String>,

        /// Path of config file.
        #[clap(short, long)]
        config_path: Option<String>,

        /// Dump the default configuration.
        #[clap(long)]
        dump_config: bool,

        /// Safe the FAI data to stateful partition of factory shim.
        #[clap(long)]
        save_to_usb: bool,
    },
    /// Do factory reset
    FactoryReset {
        /// The action to perform.
        #[clap(subcommand)]
        action: ResetAction,
    },
    BatteryCutoff,
    CustomResetProcess,
}

/// ChromeOS Factory installer process.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    /// The action to perform.
    #[clap(subcommand)]
    pub action: Action,
}

#[derive(Debug, Subcommand)]
pub enum ResetAction {
    /// Write 0's on every LBA [backward compatibility].
    Wipe,
    /// Use internal erase command in the device and write a pattern on the disk
    Secure,
    /// Verify the disk has been erased properly
    Verify,
    /// Do factory reset
    Reset,
}

impl fmt::Display for ResetAction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ResetAction::Wipe => write!(f, "wipe"),
            ResetAction::Secure => write!(f, "secure"),
            ResetAction::Verify => write!(f, "verify"),
            ResetAction::Reset => write!(f, "reset"),
        }
    }
}
