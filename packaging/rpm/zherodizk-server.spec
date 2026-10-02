Name:           zherodizk-server
Version:        @VERSION@
Release:        1
Summary:        ZHeroDiZk rendezvous and relay servers
License:        AGPL-3.0-only
URL:            https://github.com/Chistovik92/ZHeroDiZk
AutoReqProv:    no

%description
Static builds of the ZHeroDiZk rendezvous server (zhd-rendezvous), relay server
(zhd-relay) and key utility (zhd-utils), with systemd units.
Development build: not a finished product.

%prep

%build

%install
install -d %{buildroot}/usr/bin %{buildroot}/usr/lib/systemd/system %{buildroot}/etc/zherodizk %{buildroot}/var/lib/zherodizk
for b in zhd-rendezvous zhd-relay zhd-utils; do install -m 0755 @BINDIR@/$b %{buildroot}/usr/bin/$b; done
install -m 0644 @SRCDIR@/systemd/*.service %{buildroot}/usr/lib/systemd/system/
install -m 0640 @SRCDIR@/common/server.env %{buildroot}/etc/zherodizk/server.env

%pre
id -u zherodizk >/dev/null 2>&1 || {
    groupadd -r zherodizk 2>/dev/null || true
    useradd -r -g zherodizk -d /var/lib/zherodizk -s /sbin/nologin zherodizk
}
exit 0

%post
chown zherodizk:zherodizk /var/lib/zherodizk
if [ -d /run/systemd/system ]; then systemctl daemon-reload || true; fi
echo "ZHeroDiZk: edit /etc/zherodizk/server.env, then run: systemctl enable --now zhd-rendezvous zhd-relay"

%preun
if [ "$1" = 0 ] && [ -d /run/systemd/system ]; then
    systemctl stop zhd-rendezvous zhd-relay || true
    systemctl disable zhd-rendezvous zhd-relay || true
fi

%files
/usr/bin/zhd-rendezvous
/usr/bin/zhd-relay
/usr/bin/zhd-utils
/usr/lib/systemd/system/zhd-rendezvous.service
/usr/lib/systemd/system/zhd-relay.service
%config(noreplace) %attr(0640,root,zherodizk) /etc/zherodizk/server.env
%dir %attr(0750,zherodizk,zherodizk) /var/lib/zherodizk
