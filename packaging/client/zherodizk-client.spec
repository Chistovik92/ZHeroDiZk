Name:           zherodizk-client
Version:        @VERSION@
Release:        1
Summary:        ZHeroDiZk remote desktop client (test build)
License:        AGPL-3.0-only
URL:            https://github.com/Chistovik92/ZHeroDiZk
AutoReqProv:    no
Requires:       python3

%description
Test build of the ZHeroDiZk client. Not a finished or security-reviewed product: it does not
yet require managed session grants. Read /usr/share/doc/zherodizk-client/README.txt first.

%prep

%build

%install
install -d %{buildroot}/opt/zherodizk %{buildroot}/usr/bin %{buildroot}/usr/share/applications \
    %{buildroot}/usr/share/icons/hicolor/256x256/apps %{buildroot}/usr/share/doc/zherodizk-client
cp -a @BUNDLE@/. %{buildroot}/opt/zherodizk/
install -m 0755 @SRCDIR@/zherodizk-wrapper %{buildroot}/usr/bin/zherodizk
install -m 0644 @SRCDIR@/zherodizk.desktop %{buildroot}/usr/share/applications/zherodizk.desktop
install -m 0644 @PNG@ %{buildroot}/usr/share/icons/hicolor/256x256/apps/zherodizk.png
cp @ROOT@/LICENSE @ROOT@/NOTICE %{buildroot}/usr/share/doc/zherodizk-client/
cp @SRCDIR@/README-linux.txt %{buildroot}/usr/share/doc/zherodizk-client/README.txt

%files
/opt/zherodizk
/usr/bin/zherodizk
/usr/share/applications/zherodizk.desktop
/usr/share/icons/hicolor/256x256/apps/zherodizk.png
/usr/share/doc/zherodizk-client
