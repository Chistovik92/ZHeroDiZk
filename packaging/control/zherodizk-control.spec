Name:           zherodizk-control
Version:        @VERSION@
Release:        1
Summary:        ZHeroDiZk control server
License:        AGPL-3.0-only
URL:            https://github.com/Chistovik92/ZHeroDiZk
AutoReqProv:    no

%description
Accounts, multi-factor authentication, organisations, devices, access rules, audit log and
signed session grants, with a systemd unit. Needs PostgreSQL 16 and a TLS reverse proxy.
Development build: not a finished product.

%prep

%build

%install
install -d %{buildroot}/usr/bin %{buildroot}/usr/lib/systemd/system %{buildroot}/etc/zherodizk %{buildroot}/usr/share/doc/zherodizk-control
install -m 0755 @BINARY@ %{buildroot}/usr/bin/zherodizk-control
install -m 0644 @SRCDIR@/control/zherodizk-control.service %{buildroot}/usr/lib/systemd/system/
install -m 0640 @SRCDIR@/control/control.env %{buildroot}/etc/zherodizk/control.env
install -m 0644 @SRCDIR@/control/Caddyfile @SRCDIR@/control/nginx.conf %{buildroot}/usr/share/doc/zherodizk-control/

%pre
id -u zherodizk-control >/dev/null 2>&1 || {
    groupadd -r zherodizk-control 2>/dev/null || true
    useradd -r -g zherodizk-control -d /nonexistent -s /sbin/nologin zherodizk-control
}
exit 0

%post
chown root:zherodizk-control /etc/zherodizk/control.env
chmod 0640 /etc/zherodizk/control.env
if [ -d /run/systemd/system ]; then systemctl daemon-reload || true; fi
echo "ZHeroDiZk control server: edit /etc/zherodizk/control.env, then run: systemctl enable --now zherodizk-control"

%preun
if [ "$1" = 0 ] && [ -d /run/systemd/system ]; then
    systemctl stop zherodizk-control || true
    systemctl disable zherodizk-control || true
fi

%files
/usr/bin/zherodizk-control
/usr/lib/systemd/system/zherodizk-control.service
%config(noreplace) %attr(0640,root,zherodizk-control) /etc/zherodizk/control.env
%doc /usr/share/doc/zherodizk-control/Caddyfile
%doc /usr/share/doc/zherodizk-control/nginx.conf
