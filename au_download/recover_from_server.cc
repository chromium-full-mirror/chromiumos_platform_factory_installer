// Copyright (c) 2013 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#include <fstream>
#include <iostream>
#include <sstream>
#include <string>

using std::cerr;
using std::endl;
using std::string;
using std::stringstream;

const int kStatefulPartition = 1;
const int kRootfsPartition = 3;
const int kOemPartition = 8;
const int kEfiPartition = 12;
// In recovery image, the release kernel is at partition 4
// But in real OS kernel, it is at partition 2
const int kRecoveryKernel = 4;
const int kRealKernel = 2;

const string kConfigPath = "recovery.conf";
const string kImagePath = "release_image.bin";
const string kZipImagePath = "release_image.bin.zip";
const string kConfigUrl =
    "https://dl.google.com/dl/edgedl/chromeos/recovery/recovery.conf";

void error_exit(const string& error_string)
{
  cerr << "ERROR:" << error_string << endl;
  exit(1);
}

string get_output_device(int partition)
{
  // Determine output device by hardware architecture(x86 or arm)
  char arch[10];
  FILE* fp = popen("crossystem arch", "r");
  fscanf(fp, "%5s", arch);
  pclose(fp);
  stringstream device_buffer;
  string arch_string(arch);
  if (arch_string.find("x86") != string::npos) {
    device_buffer << "/dev/sda";
    if (partition >= 0) {
      device_buffer << partition;
    }
  } else if (arch_string.find("arm") != string::npos) {
    device_buffer << "/dev/mmcblk0";
    if (partition >= 0) {
      device_buffer << "p" << partition;
    }
  } else {
    // Fail to detect architecture
    error_exit("Failed to auto detect architecture.");
  }
  return device_buffer.str();
}

string mount_partition(int partition,
                       const string& mount_option="")
{
  // Mount with the specified partition with option.
  string partition_name;
  char mount_template[] = "mount_XXXXXX";
  string mount_point(mkdtemp(mount_template));
  mount_point += "/";
  partition_name = get_output_device(partition);
  stringstream mount_command;
  mount_command << "mount " << mount_option << " " << partition_name <<
                   " " << mount_point;
  cerr << mount_command.str() << endl;
  if (system(mount_command.str().c_str()) != 0) {
    error_exit("Failed to mount " + partition_name + ".");
  }
  return mount_point;
}

void unmount_partition(const string& mount_point)
{
  // Unmount stateful partition
  if (system(("umount " + mount_point).c_str()) != 0) {
    error_exit("Failed to ummount " + mount_point + ".");
  };
  // Remove temp directory
  if (remove(mount_point.c_str()) != 0) {
    error_exit("Unable to remove temporary mount point.");
  }
}

bool find_key_value(const string& line, const string& key, string& value)
{
  if (line.find(key) == 0) {
    int equal_pos = key.length();
    if (line[equal_pos] == '=') {
      value = line.substr(equal_pos + 1);
      return true;
    }
  }
  return false;
}

void initialize_partition()
{
  // Initialize partition table
  string pmbr = "/root/.pmbr_code";
  // Check if pmbr exists
  if (access(pmbr.c_str(), R_OK) != 0) {
    error_exit("Missing " + pmbr +", please rebuild image.");
  }

  string device_name = get_output_device(-1);
  if (system((". /usr/sbin/write_gpt.sh &&"
              " write_base_table " + device_name + " " + pmbr).c_str()) != 0) {
    error_exit("Cannot write partition table.");
  }
  cerr << "Reloading partition table changes..."  << endl;
  sync();
  if (system(("partprobe " + device_name).c_str()) != 0) {
    error_exit("Cannot reload partition table.");
  }
  cerr << "Make a new stateful partition." << endl;
  string stateful_partition = get_output_device(kStatefulPartition);
  if (system(("mkfs.ext4 " + stateful_partition).c_str()) != 0) {
    error_exit("Cannot make stateful partition.");
  }
  cerr << "Done preparing disk." << endl;
}

string find_board()
{
  const string kLsbRelease = "/etc/lsb-release";
  const string kBoardString = "CHROMEOS_RELEASE_BOARD";
  string buffer, board;
  std::ifstream fin;
  fin.open(kLsbRelease.c_str());
  while(std::getline(fin, buffer)) {
    if (find_key_value(buffer, kBoardString, board)) {
      break;
    }
  }
  cerr << "Board: " << board << endl;
  // Can not find board, error
  if (board.empty()) {
    error_exit("Can not find board name, installation failed.");
  }
  return board;
}

// The network interface may not be ready, so let's try multiple times here
void download_config(const string& stateful_mount)
{
  int retry = 10;
  while (retry-- > 0) {
    if (system(("wget " + kConfigUrl +
                " -O " + stateful_mount + kConfigPath +
                " --no-check-certificate").c_str()) == 0) {
      // Succeed and retrun
      return;
    }
    cerr << "Connection failed, wait 3 seconds and retry(" << retry <<
            "times left).";
    sleep(3);
  }
  // After 10 tries, return fail
  error_exit("Fail downloading recovery image configuration.");
}

void download_image(const string& stateful_mount, const string& board)
{
  std::ifstream fin;
  string buffer, filename, download_link;

  // To avoid the confusion of x86-alex and x86-alex-he,
  // change board from board to board_
  string board_ = board + "_";

  fin.open((stateful_mount + kConfigPath).c_str());
  while(std::getline(fin, buffer)) {
    if (find_key_value(buffer, "url", filename)) {
      if (filename.find(board_) != string::npos) {
        download_link = filename;
        break;
      }
    }
  }

  // Can not find download url, error
  if (download_link.empty()) {
    error_exit("Can not find download url, installation failed.");
  }

  // Download the release image
  if (system(("wget " + download_link +
              " -O " + stateful_mount + kZipImagePath +
              " --no-check-certificate").c_str()) != 0) {
    error_exit("Fail downloading release image.");
  }

  // Unzip the release image.
  // To use gzip to decompress .zip file, we assume the zip archieve contain
  // the release image and no other files.
  // /tmp/image.bin.zip -> /tmp/image.zip
  if (system(("gzip -d -f -S .zip " +
              stateful_mount + kZipImagePath).c_str()) != 0) {
    error_exit("Fail to unzip image.");
  }
}

int get_image_offset(const int partition, const string& path)
{
  // Read the offset of a partition in an image by cgpt
  FILE* fp;
  int offset;
  stringstream command;

  command << "cgpt show -b -i " << partition << " " << path;
  fp = popen(command.str().c_str(), "r");
  fscanf(fp, "%d", &offset);
  pclose(fp);

  return offset;
}

int get_image_size(const int partition, const string& path)
{
  // Read the size (by number of 512 byte sectors) of a partition in an image
  // by cgpt
  FILE* fp;
  int size;
  stringstream command;

  command << "cgpt show -s -i " << partition << " " << path;
  fp = popen(command.str().c_str(), "r");
  fscanf(fp, "%d", &size);
  pclose(fp);

  return size;
}

void install_partition(const string& stateful_mount,
                       const int partition,
                       const int dest=-1)
{
  int sector_size = 512;
  int begin_sector;
  int num_sectors;
  int dest_partition = (dest == -1) ? partition : dest;
  string image_path = stateful_mount + kImagePath;

  // Read the partition information
  begin_sector = get_image_offset(partition, image_path);
  num_sectors = get_image_size(partition, image_path);

  // We want to enlarge sector size to speed up dd
  // Increase as much as possible up to 8M
  while(begin_sector % 2 == 0 && num_sectors % 2 == 0 &&
        sector_size < 8 * 1024 * 1024 && num_sectors > 0) {
    sector_size *= 2;
    begin_sector /= 2;
    num_sectors /=2 ;
  }

  string output_device;
  output_device = get_output_device(dest_partition);

  stringstream command;
  // Generate dd command, pv is used to show progress.
  command << "dd if=" << image_path << " bs=" << sector_size <<
             " skip=" << begin_sector << " count=" << num_sectors <<
             " | pv -ptreb -B 2M -s " << (uint64_t)sector_size * num_sectors <<
             " | dd of=" << output_device << " bs=" << sector_size <<
             " iflag=fullblock oflag=dsync";

  // Install to device by dd.
  cerr << command.str().c_str() << endl;
  if (system(command.str().c_str()) != 0) {
    error_exit("Unable to install partition.");
  }
}

void install_firmware_updater()
{
  // Extract firmware updater from rootfs
  const string updater_path = "/usr/sbin/chromeos-firmwareupdate";
  string rootfs_mount;
  char tmp_name[] = "fw_XXXXXX";
  string tmp_updater(mktemp(tmp_name));
  rootfs_mount = mount_partition(kRootfsPartition, "-o ro -t ext2");
  if (system(("cp " + rootfs_mount + updater_path +
              " " + tmp_updater).c_str()) != 0) {
    error_exit("Fail to copy firmware updater.");
  }
  unmount_partition(rootfs_mount);

  // Run firmware update script
  if (system(("sh " + tmp_updater +
             " --force --mode=recovery").c_str()) != 0) {
    error_exit("Fail to run firmware updater.");
  }

  if (remove(tmp_updater.c_str()) != 0) {
    error_exit("Fail to remove temp updater.");
  }
}

void postprocess_release(const string& stateful_mount)
{
  const string kVmlinuzHdFile="vmlinuz_hd.vblock";
  string image_path = stateful_mount + kImagePath;
  char mount_template[] = "image_XXXXXX";
  string image_mount(mkdtemp(mount_template));
  image_mount += "/";

  // Find of the location of stateful partition
  int begin_sector = get_image_offset(kStatefulPartition, image_path);
  int num_sectors = get_image_size(kStatefulPartition, image_path);

  stringstream command;
  command << "mount -o loop,offset=" << (uint64_t) begin_sector * 512 <<
             ",sizelimit=" << (uint64_t) num_sectors * 512 << ",ro" <<
             " " << image_path << " " << image_mount;
  cerr << command.str() << endl;
  if (system(command.str().c_str()) != 0) {
    error_exit("Cannot mount stateful partition from image");
  }

  command.str(string());
  string kernel_device = get_output_device(kRealKernel);
  command << "dd if=" << image_mount << kVmlinuzHdFile <<
             " of=" << kernel_device << " bs=512 conv=notrunc";
  if (system(command.str().c_str()) != 0) {
    error_exit("Cannot update kernel with " + kVmlinuzHdFile);
  }

  if (system(("umount " + image_mount).c_str()) != 0) {
    error_exit("Cannot umount " + image_mount);
  }
  if (remove(image_mount.c_str()) != 0){
    error_exit("Cannot remove " + image_mount);
  }
}

void install_all_partition(const string& stateful_mount)
{
  install_partition(stateful_mount, kRecoveryKernel, kRealKernel);
  install_partition(stateful_mount, kRootfsPartition);
  install_partition(stateful_mount, kOemPartition);
  install_partition(stateful_mount, kEfiPartition);
  // Since we use release image, we need to copy vmlinuz_hd.block from
  // stateful partition to kernel partition.
  postprocess_release(stateful_mount);
}

int main()
{
  string board, stateful_mount;

  initialize_partition();
  stateful_mount = mount_partition(kStatefulPartition);
  board = find_board();
  download_config(stateful_mount);
  download_image(stateful_mount, board);
  install_all_partition(stateful_mount);
  unmount_partition(stateful_mount);
  install_firmware_updater();
  return 0;
}
