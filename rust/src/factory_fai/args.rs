// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub use clap::Parser;

/// ChromeOS Factory First Article Inspection Process.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    /// Output the collected data to the file path
    #[clap(short, long)]
    pub output_path: Option<String>,

    /// Path of config file.
    #[clap(short, long)]
    pub config_path: Option<String>,

    /// Dump the default configuration.
    #[clap(long)]
    pub dump_config: bool,

    /// Safe the FAI data to stateful partition of factory shim.
    #[clap(long)]
    pub save_to_usb: bool,
}
