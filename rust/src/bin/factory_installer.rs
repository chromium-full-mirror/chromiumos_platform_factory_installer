// Copyright 2023 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use anyhow::Result;
use factory_installer::cutoff;
use factory_installer::factory_fai;
use factory_installer::factory_installer::args::{Action, Args, Parser};
use factory_installer::factory_installer::factory_reset;
use factory_installer::factory_installer::script_wrapper;
use factory_installer::system::context::ContextImpl;

fn main() -> Result<()> {
    let args = Args::parse();
    let mut context = ContextImpl::new();
    // TODO(b/320872246): No need to patch after the issue is solved.
    script_wrapper::call_script_wrapper(vec!["cros_payload_patch_cutoff_config"], &mut context)?;
    script_wrapper::call_script_wrapper(vec!["cros_payload_patch_lsb_factory"], &mut context)?;
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
        Action::FactoryReset { action } => factory_reset::factory_reset(action, &mut context),
        Action::BatteryCutoff => cutoff::do_cutoff(&mut context),
    }
}
