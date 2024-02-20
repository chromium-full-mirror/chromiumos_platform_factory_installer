// Copyright 2024 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use anyhow::Result;
use image::Luma;
use png::Encoder;
use qrcode::QrCode;

use crate::system::context::Context;

fn get_frecon_path(context: &mut dyn Context) -> Result<PathBuf> {
    let frecon_pid = fs::read_to_string(context.root_dir().join("run/frecon/pid"))?;
    Ok(context
        .root_dir()
        .join("proc")
        .join(frecon_pid)
        .join("root"))
}

/// Displays the qrcode by frecon console with the given content.
pub fn display_qrcode(content: &str, context: &mut dyn Context) -> Result<()> {
    let frecon_path = get_frecon_path(context)?;
    let file = File::create(frecon_path.join("qrcode.png").to_str().unwrap()).unwrap();

    let image = QrCode::new(content)?.render::<Luma<u8>>().build();
    let encoder = Encoder::new(BufWriter::new(file), image.width(), image.height());
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&image).unwrap();

    let terminal_path = context.root_dir().join("run/frecon/vt0");
    Ok(fs::write(
        terminal_path,
        "\x1b]image:file=/qrcode.png\x1b\\",
    )?)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};

    use crate::system::context::{Context, ContextImpl};
    use crate::utils::qrcode_utils;

    #[test]
    fn test_display_qrcode() {
        let frecon_id = "frecon_id";
        let mut context = ContextImpl::new();
        fs::create_dir_all(context.root_dir().join("proc").join(frecon_id).join("root")).unwrap();
        let dir = context.root_dir().join("run/frecon");
        fs::create_dir_all(dir.clone()).unwrap();
        File::create(dir.join("vt0")).unwrap();
        fs::write(dir.join("pid"), frecon_id).unwrap();

        let result = qrcode_utils::display_qrcode("HWID", &mut context);
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

        let result = qrcode_utils::display_qrcode("HWID", &mut context);
        let err = result.unwrap_err();
        let message = err.root_cause();
        assert_eq!(
            format!("{}", message),
            "No such file or directory (os error 2)"
        );
    }
}
