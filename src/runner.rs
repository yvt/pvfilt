use std::{
    ffi::OsString,
    process::{Command, ExitStatus, Stdio},
    time::Duration,
};

pub type CmdResult = Result<CmdOutput, std::io::Error>;

pub struct CmdOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

pub fn watch_cmd(cmd: Vec<OsString>, mut cb: impl FnMut(CmdResult)) {
    loop {
        let child = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdout(Stdio::piped())
            .spawn();

        let output = child.and_then(|child| child.wait_with_output());

        cb(output.map(|output| CmdOutput {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }));

        std::thread::sleep(Duration::from_secs(1));
    }
}
