// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use factory_installer::factory_installer::args::{Action, Args, Parser};
use factory_installer::factory_installer::{do_custom_reset_process, factory_reset};
use factory_installer::system::context::ContextImpl;
use factory_installer::{cutoff, factory_fai};

fn main() -> Result<()> {
    let args = Args::parse();
    let mut context = ContextImpl::new();
    match args.action {
        Action::FAI {
            output_path,
            config_path,
            dump_config,
            save_to_usb,
        } => factory_fai::perform_fai(
            &mut context,
            output_path,
            config_path,
            dump_config,
            save_to_usb,
        ),
        Action::FactoryReset { action } => {
            factory_reset::factory_reset(action, &mut context)?;
            cutoff::do_cutoff(&mut context)?;
            cutoff::failed(&mut context)
        }
        Action::BatteryCutoff => {
            cutoff::do_cutoff(&mut context)?;
            cutoff::failed(&mut context)
        }
        Action::CustomResetProcess => {
            do_custom_reset_process(&mut context)?;
            cutoff::failed(&mut context)
        }
    }
}
