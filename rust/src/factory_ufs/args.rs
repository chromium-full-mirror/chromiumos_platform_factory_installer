// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub use clap::{Parser, Subcommand};

const PCI_RESCAN_DEFAUT_TIMEOUT_SEC: u32 = 10;
const PURGE_DEFAULT_TIMEOUT_SEC: u32 = 300;

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Perform purge to clean up the discarded blocks.
    Purge {
        /// Timeout limit for the purge operation.
        #[clap(short, long, default_value_t=PURGE_DEFAULT_TIMEOUT_SEC)]
        timeout: u32,
    },
    /// Print the current UFS config on DUT
    Show,
    /// Provision the UFS
    Provision {
        /// Timeout seconds for PCI rescan
        #[clap(short, long, default_value_t=PCI_RESCAN_DEFAUT_TIMEOUT_SEC)]
        rescan_timeout: u32,

        /// Force re-provisioning even if the UFS config is the same.
        #[clap(short, long)]
        force: bool,
    },
    /// Provision the UFS with descriptor files
    ProvisionFile {
        /// The configuration descriptor file.
        #[clap(short = 'c', long)]
        input_config_descriptor: String,
        /// The device descriptor file.
        #[clap(short = 'd', long)]
        input_device_descriptor: String,
        /// The geometry descriptor file.
        #[clap(short = 'g', long)]
        input_geometry_descriptor: String,
        /// The path to write the new, modified configuration descriptor.
        #[clap(short = 'o', long)]
        output_file: String,
    },
}

/// ChromeOS Factory UFS.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about=None)]
pub struct Args {
    /// Operation to perform.
    #[clap(subcommand)]
    pub command: Commands,
}
