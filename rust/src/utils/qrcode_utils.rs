// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs;
use std::io::BufWriter;
use std::path::PathBuf;

use anyhow::Result;
use image::Luma;
use png::Encoder;
use qrcode::QrCode;
use serde::Deserialize;

use crate::system::context::Context;
#[cfg(not(test))]
use crate::utils::sys_utils;
#[cfg(test)]
use crate::utils::sys_utils::mock as sys_utils;

#[derive(PartialEq, Deserialize, Debug)]
pub struct QRcode {
    pub size: u32,
    pub position: Option<(i32, i32)>,
    pub content: String,
}

fn get_frecon_path(context: &mut dyn Context) -> Result<PathBuf> {
    let frecon_pid = fs::read_to_string(context.root_dir().join("run/frecon/pid"))?;
    Ok(context
        .root_dir()
        .join("proc")
        .join(frecon_pid)
        .join("root"))
}

/// Displays the qrcode by frecon console with the given content, size, location.
pub fn display_qrcode(qrcode: &QRcode, context: &mut dyn Context) -> Result<()> {
    let frecon_path = get_frecon_path(context)?;
    let (file, path) = context.tempfile_in(&frecon_path)?;
    let name = path.file_name().unwrap().to_str().unwrap();
    let image = QrCode::new(&qrcode.content)?
        .render::<Luma<u8>>()
        .max_dimensions(qrcode.size, qrcode.size)
        .build();
    let encoder = Encoder::new(BufWriter::new(file), image.width(), image.height());
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&image).unwrap();
    let terminal_path = context.root_dir().join("run/frecon/vt0");
    let frecon_string = match qrcode.position {
        None => format!("\x1b]image:file=/{}\x1b\\", name),
        Some(p) => format!("\x1b]image:file=/{};location={},{}\x1b\\", name, p.0, p.1),
    };
    // TODO(jasonchuang): We need to consult with frecon team when will the
    // qrcode be cleared, sleep a while as a workaround.
    sys_utils::sleep(0.2)?;
    Ok(fs::write(terminal_path, frecon_string)?)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};

    use crate::system::context::{Context, ContextImpl};
    use crate::utils::qrcode_utils::{self, QRcode};

    #[test]
    fn test_display_qrcode() {
        let frecon_id = "frecon_id";
        let mut context = ContextImpl::new();
        fs::create_dir_all(context.root_dir().join("proc").join(frecon_id).join("root")).unwrap();
        let dir = context.root_dir().join("run/frecon");
        fs::create_dir_all(dir.clone()).unwrap();
        File::create(dir.join("vt0")).unwrap();
        fs::write(dir.join("pid"), frecon_id).unwrap();
        context.set_tempfile("qrcode.png".to_string());

        let result = qrcode_utils::display_qrcode(
            &QRcode {
                size: 100,
                content: "HWID".to_string(),
                position: None,
            },
            &mut context,
        );
        assert!(result.is_ok());
        assert_eq!(
            fs::read(
                context
                    .root_dir()
                    .join("proc/frecon_id/root/qrcode.png")
                    .to_str()
                    .unwrap()
            )
            .unwrap(),
            fs::read("tests/qrcode/HWID.png").unwrap()
        );
        let content = fs::read_to_string(context.root_dir().join("run/frecon/vt0")).unwrap();
        assert_eq!(content, "\x1b]image:file=/qrcode.png\x1b\\");
    }

    #[test]
    fn test_display_qrcode_fail() {
        let mut context = ContextImpl::new();
        context.set_command_stdout("cat", "non-existing-path".to_string());

        let result = qrcode_utils::display_qrcode(
            &QRcode {
                size: 232,
                content: "HWID".to_string(),
                position: None,
            },
            &mut context,
        );
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(
            format!("{}", message),
            "No such file or directory (os error 2)"
        );
    }
}
