use nix;
use nix::NixPath;
use nix::mount::{self, MsFlags};

pub const NONE: Option<&'static [u8]> = None;

pub fn bind_mount<P1: ?Sized + NixPath, P2: ?Sized + NixPath>(
    source: &P1,
    target: &P2,
) -> nix::Result<()> {
    mount::mount(
        Some(source),
        target,
        NONE,
        MsFlags::MS_REC | MsFlags::MS_BIND,
        NONE,
    )
}

pub fn mount_private<P: ?Sized + NixPath>(path: &P) -> nix::Result<()> {
    mount::mount(
        Some("none"),
        path,
        NONE,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        NONE,
    )
}

pub fn move_mounts<P1: ?Sized + NixPath, P2: ?Sized + NixPath>(
    source: &P1,
    target: &P2,
) -> nix::Result<()> {
    mount::mount(
        Some(source),
        target,
        NONE,
        MsFlags::MS_REC | MsFlags::MS_MOVE,
        NONE,
    )
}
