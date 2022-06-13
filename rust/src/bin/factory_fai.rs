// Copyright 2022 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs;

use factory_installer::factory_fai;
use factory_installer::factory_fai::args::{Args, Parser};
use factory_installer::factory_fai::config;

use anyhow::{Context, Result};

fn main() -> Result<()> {
    let args = Args::parse();

    if args.dump_config {
        println!("{}", config::DEFAULT_CONFIG);
        return Ok(());
    }

    let fai_config = config::load_config(args.config_path).context("Failed to load config.")?;

    let data = factory_fai::collect_fai_data(fai_config)?;

    println!("{}", &data);
    if args.save_to_usb {
        let (dev, filename) = factory_fai::save_to_usb(&data)?;
        eprintln!("Succeed to save {} in {}.", filename, dev);
    }

    if let Some(path) = args.output_path {
        eprintln!("Writing result to {}.", path);
        fs::write(path, &data).context("Failed to write result to output file.")?;
    }
    Ok(())
}
