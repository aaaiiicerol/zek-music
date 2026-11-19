#!/usr/bin/env python3
"""Adiciona o STORE_BUILD ao src/main.rs: com STORE_BUILD=true na compilacao,
o programa nao consulta o GitHub e esconde a secao de atualizacoes dos Ajustes.
Seguro para rodar de novo (nao aplica duas vezes)."""
import sys

p = "src/main.rs"
s = open(p, encoding="utf-8").read()

if "pub const STORE_BUILD" in s:
    print("Ja aplicado.")
    sys.exit(0)

falhas = []

def troca(nome, old, new):
    global s
    if old in s:
        s = s.replace(old, new, 1)
        print("ok   :", nome)
    else:
        falhas.append(nome)
        print("FALTA:", nome)

# 1) Adiciona a constante STORE_BUILD antes de fn config_path
troca(
    "constante STORE_BUILD",
    "fn config_path() -> PathBuf {",
    '/// Build da Microsoft Store: compile com a variavel de ambiente STORE_BUILD=true.\npub const STORE_BUILD: bool = matches!(option_env!("STORE_BUILD"), Some("true"));\n\nfn config_path() -> PathBuf {'
)

# 2) Desativa a verificacao de atualizacao ao abrir
troca(
    "verificacao ao abrir",
    "if app.cfg.check_updates && app.due_update_check() {",
    "if app.cfg.check_updates && !STORE_BUILD && app.due_update_check() {"
)

# 3) Esconde a secao de atualizacoes nos Ajustes
old_settings = """                ui.add_space(8.0);
                let mut bandeira = self.cfg.check_updates;
                if ui
                    .checkbox(&mut bandeira, "Avisar quando sair vers\u00e3o nova (consulta o GitHub 1 vez ao dia)")
                    .changed()
                {
                    acts.push(Act::SetCheckUpdates(bandeira));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Verificar agora").clicked() {
                        acts.push(Act::CheckUpdates);
                    }
                    if ui.button("Abrir pasta do registro").clicked() {
                        acts.push(Act::OpenDataDir);
                    }
                });
                ui.add_space(10.0);"""

new_settings = """                ui.add_space(8.0);
                if !STORE_BUILD {
                    let mut bandeira = self.cfg.check_updates;
                    if ui
                        .checkbox(&mut bandeira, "Avisar quando sair vers\u00e3o nova (consulta o GitHub 1 vez ao dia)")
                        .changed()
                    {
                        acts.push(Act::SetCheckUpdates(bandeira));
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Verificar agora").clicked() {
                            acts.push(Act::CheckUpdates);
                        }
                        if ui.button("Abrir pasta do registro").clicked() {
                            acts.push(Act::OpenDataDir);
                        }
                    });
                }
                ui.add_space(10.0);"""

troca("secao Atualizacoes nos Ajustes", old_settings, new_settings)

if falhas:
    print("\nAplicado com pendencias:", falhas)
else:
    open(p, "w", encoding="utf-8").write(s)
    print("\nPronto. Patch aplicado com sucesso.")
