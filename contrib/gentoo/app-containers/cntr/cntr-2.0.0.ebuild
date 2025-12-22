# Copyright 2024 Gentoo Authors
# Distributed under the terms of the GNU General Public License v2

EAPI=8

CRATES=""

inherit cargo

DESCRIPTION="A container debugging tool based on Linux mount API"
HOMEPAGE="https://github.com/Mic92/cntr"
SRC_URI="https://github.com/Mic92/cntr/archive/refs/tags/${PV}.tar.gz -> ${P}.tar.gz
	${CARGO_CRATE_URIS}"

LICENSE="MIT"
# Dependent crate licenses
LICENSE+=" MIT"
SLOT="0"
KEYWORDS="~amd64 ~arm64"

BDEPEND=">=dev-lang/rust-1.85
	app-text/scdoc"

QA_FLAGS_IGNORED="usr/bin/cntr"

src_compile() {
	cargo_src_compile
	scdoc < doc/cntr.1.scd > cntr.1
}

src_install() {
	cargo_src_install
	dodoc README.md
	newdoc LICENSE.md LICENSE
	doman cntr.1

	# Shell completions
	newbashcomp completions/cntr.bash cntr
	insinto /usr/share/zsh/site-functions
	newins completions/cntr.zsh _cntr
	insinto /usr/share/fish/vendor_completions.d
	doins completions/cntr.fish
	insinto /usr/share/nushell/completions
	doins completions/cntr.nu
}
