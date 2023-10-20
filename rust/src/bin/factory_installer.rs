// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use factory_installer::factory_fai;
use factory_installer::factory_installer::args::{Action, Args, Parser};

fn main() -> Result<()> {
    let args = Args::parse();
    match args.action {
        Action::FAI {
            output_path,
            config_path,
            dump_config,
            save_to_usb,
        } => factory_fai::perform_fai(output_path, config_path, dump_config, save_to_usb),
    }
}
