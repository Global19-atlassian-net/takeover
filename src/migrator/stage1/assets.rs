use std::fs::{write, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use failure::ResultExt;
use log::{error, Level};

use crate::{
    common::{
        call,
        defs::CHMOD_CMD,
        mig_error::{MigErrCtx, MigError, MigErrorKind},
    },
    stage1::defs::OSArch,
};

const RPI3_BUSYBOX: &[u8] = include_bytes!("../../../assets/armv7/busybox");
const X86_64_BUSYBOX: &[u8] = include_bytes!("../../../assets/x86_64/busybox");

const STAGE2_SCRIPT: &str = r###"#!__TO__/busybox sh
echo "takeover init started"
if [ -f "__TO____TTY__" ]; then 
  exec <"__TO____TTY__" >"__TO____TTY__" 2>"__TO____TTY__"
fi
cd "__TO__"
echo "Init takeover successful"
echo "Pivoting root..."
mount --make-rprivate /
pivot_root . mnt/old_root
echo "Chrooting and running init..."
exec ./busybox chroot . /takeover --init --s2-log-level __LOG_LEVEL__
"###;

#[derive(Debug)]
pub(crate) struct Assets {
    arch: OSArch,
    busybox: &'static [u8],
}

impl Assets {
    pub fn new() -> Assets {
        if cfg!(target_arch = "arm") {
            Assets {
                arch: OSArch::ARMHF,
                busybox: RPI3_BUSYBOX,
            }
        } else if cfg!(target_arch = "x86_64") {
            Assets {
                arch: OSArch::AMD64,
                busybox: X86_64_BUSYBOX,
            }
        } else {
            panic!("No assets are provided in binary - please compile with device feature")
        }
    }

    pub fn write_stage2_script<P1: AsRef<Path>, P2: AsRef<Path>, P3: AsRef<Path>>(
        to_dir: P1,
        out_path: P2,
        tty: P3,
        log_level: Level,
    ) -> Result<(), MigError> {
        let s2_script = STAGE2_SCRIPT.replace("__TO__", &*to_dir.as_ref().to_string_lossy());
        let s2_script = s2_script.replace("__TTY__", &*tty.as_ref().to_string_lossy());
        let s2_script = s2_script.replace("__LOG_LEVEL__", log_level.to_string().as_str());
        write(out_path.as_ref(), &s2_script).context(upstream_context!(&format!(
            "Failed to write stage 2 script to: '{}'",
            out_path.as_ref().display()
        )))?;
        let cmd_res = call(
            CHMOD_CMD,
            &["+x", &*out_path.as_ref().to_string_lossy()],
            true,
        )?;
        if cmd_res.status.success() {
            Ok(())
        } else {
            error!(
                "Failed to set executable flags on stage 2 script: '{}', stderr: '{}'",
                out_path.as_ref().display(),
                cmd_res.stderr
            );
            Err(MigError::displayed())
        }
    }

    #[allow(dead_code)]
    pub fn get_os_arch(&self) -> &OSArch {
        &self.arch
    }

    pub fn busybox_size(&self) -> usize {
        self.busybox.len()
    }

    pub fn write_to<P: AsRef<Path>>(&self, target_path: P) -> Result<PathBuf, MigError> {
        let target_path = target_path.as_ref().join("busybox");

        {
            let mut target_file = OpenOptions::new()
                .create(true)
                .write(true)
                .read(false)
                .open(&target_path)
                .context(upstream_context!(&format!(
                    "Failed to open file for writing: '{}'",
                    target_path.display()
                )))?;
            target_file
                .write(self.busybox)
                .context(upstream_context!(&format!(
                    "Failed to write to file: '{}'",
                    target_path.display()
                )))?;
        }

        /*
        let mut busybox_file = OpenOptions::new().create(false).write(true).open(&target_path)
            .context(MigErrCtx::from_remark(upstream_context!(
                                            &format!("Failed to set open '{}'", target_path.display())))?;

        let metadata = busybox_file.metadata()
            .context(upstream_context!(
                                            &format!("Failed to get metadata for '{}'", target_path.display())))?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        */

        let cmd_res = call(CHMOD_CMD, &["+x", &*target_path.to_string_lossy()], true)?;

        if !cmd_res.status.success() {
            return Err(MigError::from_remark(
                MigErrorKind::CmdIO,
                &format!(
                    "Failed to set executable flags for '{}', stderr: '{}'",
                    target_path.display(),
                    cmd_res.stderr
                ),
            ));
        }

        Ok(target_path)
    }
}
