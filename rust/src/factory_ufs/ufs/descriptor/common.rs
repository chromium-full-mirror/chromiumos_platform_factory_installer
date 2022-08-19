// Copyright 2022 The ChromiumOS Authors.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::fmt::Debug;

use serde::{Deserialize, Serialize};

pub trait CommonDescriptorTrait<'de>:
    Serialize + Deserialize<'de> + Default + Debug + PartialEq
{
}
