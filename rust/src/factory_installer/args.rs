// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

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
}

/// ChromeOS Factory installer process.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    /// The action to perform.
    #[clap(subcommand)]
    pub action: Action,
}
