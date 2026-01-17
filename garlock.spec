Name:           garlock
Version:        0.1.0
Release:        1%{?dist}
Summary:        Screen locker for the gar desktop suite

License:        MIT
URL:            https://github.com/gardesk/garlock
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  rust >= 1.75
BuildRequires:  cargo
BuildRequires:  libxcb-devel
BuildRequires:  cairo-devel
BuildRequires:  pango-devel
BuildRequires:  pam-devel

Requires:       pam

# Disable debug package
%global debug_package %{nil}

%description
Garlock is a screen locker for X11 built in Rust. Features blur effects,
clock display, and PAM authentication. Part of the gardesk desktop environment
suite.

%prep
%autosetup

%build
export CARGO_TARGET_DIR=target
cargo build --release --workspace

%install
install -Dm755 target/release/garlock %{buildroot}%{_bindir}/garlock

%files
%{_bindir}/garlock

%changelog
* Fri Jan 17 2025 mfw <espadonne@outlook.com> - 0.1.0-1
- Initial RPM release of garlock
- Screen locker with blur and PAM auth
- Part of gardesk desktop suite
