// Copyright 2022 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use factory_installer::factory_fai;
use factory_installer::factory_fai::args::{Args, Parser};
use factory_installer::system::context::ContextImpl;

// TODO(jasonchuang): Remove this file after we make all users
// use factory_installer instead of this binary.
fn main() -> Result<()> {
    let args = Args::parse();
    let mut context = ContextImpl::new();

    factory_fai::perform_fai(
        &mut context,
        args.output_path,
        args.config_path,
        args.dump_config,
        args.save_to_usb,
    )
}
