// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

pub use clap::{Parser, Subcommand};

use crate::factory_ufs::ufs_spec::ufs_common;

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Perform clobber
    Clobber,
    /// Print the current UFS config on DUT
    Show,
    /// Provision the UFS
    Provision {
        /// Timeout seconds for PCI rescan
        #[clap(short, long, default_value_t=ufs_common::PCI_RESCAN_TIMEOUT_SEC)]
        rescan_timeout: u32,
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
