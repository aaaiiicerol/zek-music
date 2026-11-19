# não gera o subpacote de depuração (não temos fontes para ele)
%global debug_package %{nil}

Name:           zek-music
Version:        0.6.0
Release:        1
Summary:        Player de músicas leve onde cada pasta vira uma playlist
Summary(pt_BR): Player de músicas leve onde cada pasta vira uma playlist

License:        MIT
URL:            https://github.com/aaaiiicerol/zek-music
Source0:        zek-music-%{version}.tar.gz

BuildArch:      x86_64
# o eframe carrega estas bibliotecas em tempo de execução (dlopen),
# então o dnf não as detecta sozinho: é preciso declará-las aqui.
Requires:       alsa-lib
Requires:       dbus-libs
Requires:       libwayland-client
Requires:       libwayland-cursor
Requires:       libwayland-egl
Requires:       libxkbcommon
Requires:       mesa-libGL

%description
O Zek Music é um player de músicas offline feito em Rust, pensado para quem
organiza a biblioteca em pastas: cada subpasta com músicas vira uma playlist
automaticamente. Ele acompanha as mudanças nas pastas enquanto você joga.

%description -l pt_BR
O Zek Music é um player de músicas offline feito em Rust, pensado para quem
organiza a biblioteca em pastas: cada subpasta com músicas vira uma playlist
automaticamente. Ele acompanha as mudanças nas pastas enquanto você joga.

%prep
%autosetup -n zek-music-%{version}

%install
install -Dm0755 zek-music %{buildroot}%{_bindir}/zek-music
install -Dm0644 packaging/linux/zek-music.desktop %{buildroot}%{_datadir}/applications/zek-music.desktop
install -Dm0644 packaging/zek-music.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/zek-music.svg
install -Dm0644 packaging/zek-music.png %{buildroot}%{_datadir}/icons/hicolor/512x512/apps/zek-music.png

%post
update-desktop-database %{_datadir}/applications 2>/dev/null || :
update-mime-database %{_datadir}/mime 2>/dev/null || :
xdg-mime default zek-music.desktop audio/mpeg audio/flac audio/ogg audio/wav audio/mp4 2>/dev/null || :
xdg-icon-resource forceupdate 2>/dev/null || :
touch %{_datadir}/icons/hicolor >/dev/null 2>&1 || :

%postun
update-desktop-database %{_datadir}/applications 2>/dev/null || :

%files
%license LICENSE
%{_bindir}/zek-music
%{_datadir}/applications/zek-music.desktop
%{_datadir}/icons/hicolor/scalable/apps/zek-music.svg
%{_datadir}/icons/hicolor/512x512/apps/zek-music.png

%changelog
* Thu Nov 19 2026 aaaiiicerol <270046641+aaaiiicerol@users.noreply.github.com> - 0.6.0
- Ordem natural das faixas e pelo número da etiqueta
- Playlists de pastas desmontadas são preservadas
- Retoma de onde parou, registro de erros, capa em segundo plano
- Cor de destaque escolhível, aviso de nova versão
- Arrastar playlists para reordenar e "Abrir com o Zek Music"