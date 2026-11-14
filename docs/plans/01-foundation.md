# 01 — Base e física

- [x] Identificar versões atuais e compatibilidade Bevy/Renet/Rapier.
- [x] Atualizar dependências e corrigir APIs incompatíveis.
- [x] Validar `cargo check --workspace`.
- [x] Validar build e testes antes de implementar gameplay.

Evidência: `cargo build --workspace -j 4` e `cargo test --workspace -j 4` aprovados, incluindo regressão para buffers desalinhados do Renet/rkyv. Servidor e cliente real permaneceram ativos por 8 s conectados, sem panic, usando Vulkan na Radeon RX 6600 XT (2026-11-14).

Rapier já existe. Reutilizar cápsulas, controlador cinemático e ray casts. Os recursos agora são componentes únicos no Bevy 0.20; separar `LocalInput` de `ClientInput` e `PlayerId` de `LocalPlayer`.
