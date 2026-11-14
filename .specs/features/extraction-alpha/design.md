# Arquitetura do super alfa

- **Física existente:** Rapier já resolve cápsulas, chão, autostep, crouch e ray casts. Mantê-lo; não adicionar outro motor.
- **common/combat:** loadout, munições, partes do corpo, colete/capacete, dano e resistência dos materiais. Tipos de rede continuam serializados exclusivamente por `common::data`.
- **common/raid:** fases Lobby/Countdown/Active/Debrief, participantes Waiting/Active/Extracted/Dead/Missing, coleta e extração. Regras pequenas testáveis sem janela.
- **common/map:** geometria única consumida como colisores no servidor e meshes no cliente; posições de caixas e zona de saída compartilhadas.
- **server/tick:** rede → comandos de lobby/partida → IA → movimento → armas → projéteis → resultado → snapshot. Nenhum dano, teleporte ou crédito vindo do cliente.
- **client:** input condicionado à fase, lobby/HUD textual, modelos simples identificáveis, mensagens de conexão e resultado. Snapshot é a fonte do equipamento confirmado e do resultado.
- **Verificação:** testes de domínio, integração ECS com Rapier e playtest automático de cliente renderizado sobre UDP real. Capturas do próprio Bevy, sem mock visual.
- **Balística alfa:** ray cast contínuo evita tunnelling. Espessura de cobertura vem do ray cast reverso no colisor atingido; cada superfície desconta energia uma vez por projétil. Concreto é impenetrável. Corpo termina a trajetória.
- **Limites:** sem persistência em disco, interpolação/predição, autenticação pública ou IA móvel. Códigos de saída e logs do playtest são gates, não alegações manuais.
