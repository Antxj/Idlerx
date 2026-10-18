<p align="center">
  <img src="docs/logo.png" width="112" alt="Ícone do Idlerx">
</p>

<h1 align="center">Idlerx</h1>

<p align="center"><b>Todas as suas contas, uma janela só.</b><br>
Várias contas do mesmo jogo de navegador abertas ao mesmo tempo, cada uma
isolada, lado a lado, leves, com VPN por grupo e macros.</p>

<p align="center"><a href="README.md">English</a> | <b>Português</b></p>

<p align="center">
  <a href="https://github.com/Antxj/Idlerx/releases/latest"><img src="https://img.shields.io/github/v/release/Antxj/Idlerx?label=Baixar%20gr%C3%A1tis&style=for-the-badge&color=14b8a6" alt="Baixar a versão mais recente"></a>
</p>

<p align="center">
  Gratuito · Código aberto · Sem anúncios · Sem cadastro · Um único arquivo, sem instalação
</p>

| Tema escuro | Tema claro |
|---|---|
| ![Quatro contas numa grade 2×2, tema escuro](docs/pt-BR/home_dark.png) | ![A mesma grade, tema claro](docs/pt-BR/home_light.png) |

<sub>Jogos nas fotos: [A Dark Room](https://github.com/doublespeakgames/adarkroom),
[Trimps](https://github.com/Trimps/Trimps.github.io) e
[Candy Box 2](https://github.com/candybox2/candybox2.github.io), todos de
código aberto. Duas contas de "Dark Room" rodam lado a lado, cada uma com o
próprio progresso.</sub>

## Por que o Idlerx

Jogar com várias contas no navegador costuma ser uma bagunça: abas anônimas,
perfis do Chrome, uma conta derrubando a outra, o PC pesando. O Idlerx
resolve isso:

- 🔒 **Cada conta isolada de verdade**: login, cookies e progresso próprios.
  Entre em várias contas do mesmo jogo ao mesmo tempo, sem uma derrubar a
  outra.
- 🪟 **Tudo numa janela**: uma conta por vez ou todas lado a lado, em grade.
- 🌐 **VPN por grupo**: cada grupo de contas sai pela sua VPN, com kill
  switch, sem mexer no resto do computador.
- 🖱️ **Macros que não roubam o seu mouse**: grave jogando e a conta repete
  seus cliques e teclas enquanto você usa o PC normalmente.
- 🍃 **Leve**: os jogos não precisam de 60 quadros por segundo nem de toda a
  sua memória.
- 🌙 **Sempre rodando**: minimize ou mande para a bandeja e os jogos
  continuam.

## Recursos

### Contas e organização

- **Uma por vez ou em grade**: grade automática que se ajusta à janela, ou
  formato fixo (lado a lado, empilhado, 2×2, 3×2, 3×3… ou quantas colunas e
  linhas quiser). Arraste uma conta da lista para um quadro da grade.
- **Grupos**: separe as contas por jogo, servidor ou o que preferir. Clique
  num grupo para ver só as contas dele na grade; abra ou pause o grupo
  inteiro de uma vez; arraste contas e grupos para reorganizar.
- **Modo foco** (F11): só o jogo, sem lista nem barra.
- **Som e zoom por conta**, barra de endereço, favoritos, renomear,
  reordenar arrastando.
- **Senhas salvas por conta**, criptografadas pelo Windows, sem
  sincronizar com nada.
- **Pausar** uma conta libera toda a memória dela, e ela continua logada
  quando você abre de novo.
- **Modo InPrivate**, num grupo inteiro ou numa conta avulsa: a conta não
  guarda nada (cookies, login, senhas, dados do site) e sempre abre numa
  página em branco. Ao pausar ou fechar o Idlerx, tudo some. Atenção: jogos
  que salvam o progresso no navegador perdem esse progresso.
- **Reabre tudo como estava**: as contas que estavam abertas voltam no
  último endereço.

<p align="center"><img src="docs/pt-BR/grid_menu.png" width="420" alt="Seletor do formato da grade"></p>

### VPN por grupo

Escolha um arquivo `.ovpn` (OpenVPN) num grupo e as contas dele passam a
acessar a internet **somente pela VPN**:

- **Kill switch**: se a VPN cair, as contas do grupo ficam sem internet até
  ela voltar. Nunca saem pela sua conexão.
- **Sem vazamentos**: DNS pela VPN, WebRTC sem revelar o seu IP, IPv6
  bloqueado. Conferido no browserleaks.com.
- **O resto do PC não muda**: só o grupo usa a VPN; os outros grupos e os
  seus outros programas continuam na sua conexão normal.
- **Uma VPN por grupo, várias ao mesmo tempo.**
- Login por certificado, usuário e senha ou **token do Google
  Authenticator** (o código de 6 dígitos é gerado sozinho).
- O grupo mostra o **IP** que os sites veem e o **download e upload**. Ao
  abrir o Idlerx, as contas esperam a VPN conectar antes de abrir.
- O instalador oficial do OpenVPN vem junto: se faltar, é só um clique.

Botão direito no grupo › **Editar (nome, VPN)…**

### Macros

Repita cliques e teclas numa conta, **sem usar o mouse nem o teclado do
Windows**: você continua usando o computador normalmente, e a macro
funciona até com a conta escondida, minimizada ou na bandeja.

- **Grave jogando**: clique em **Gravar macro** e use o jogo normalmente.
  Seus cliques, teclas e o tempo entre eles viram a macro, prontos para
  ajustar.
- **Ou monte numa foto da tela**: clique onde a macro deve clicar. Botão
  esquerdo ou direito, simples ou duplo.
- **Teclas e esperas**: letras, números, F1–F12, setas, Espaço, Enter,
  Esc…
- **Repetição**: a cada quantos segundos, quantas vezes ou até você parar.
- **Pontos em proporção da tela**: continuam no lugar se o quadro mudar de
  tamanho.
- **Para sozinha** se a página recarregar, a conta fechar ou a VPN cair.
  E o ícone da bandeja tem **Parar todas as macros**.

Botão direito na conta › **Gravar macro** ou **Macro…**

### Leve e sempre rodando

- **Limite de quadros por segundo**: por padrão 60 na conta selecionada e 30
  nas outras (ajustável, inclusive por conta). Com quatro jogos animados na
  grade, a CPU caiu de 25% para 6%.
- **Modo econômico** (ligado por padrão): as contas compartilham o
  navegador, com cerca de 40% menos memória.
- **Fora da tela, sem desperdício**: contas escondidas não desenham e usam
  menos memória, mas o jogo continua: timers sem desaceleração e o loop do
  jogo rodando a 5 quadros por segundo. Minimizado, o Idlerx gasta cerca de
  dez vezes menos CPU.
- **Bandeja do sistema**: o X manda para a bandeja e os jogos seguem. O
  ícone fica com um ponto vermelho se a VPN de um grupo cair, e o Windows
  avisa quando cai e quando volta.
- **Se recupera sozinho**: uma página que trava recarrega sozinha.
- **CPU e RAM** de cada conta, de cada grupo e do total, sempre à vista.
- **Registro das páginas**: quando cada conta carregou, recarregou (e por
  quê), ficou escondida ou visível. Ótimo para entender o que aconteceu
  enquanto você estava fora.

<p align="center"><img src="docs/pt-BR/settings.png" width="420" alt="Configurações"></p>

## Comece em 1 minuto

1. **Baixe** o `Idlerx.exe` na página de [Releases](../../releases/latest).
2. **Abra**. Na primeira vez, o Windows pode mostrar "aplicativo não
   reconhecido": clique em **Mais informações** › **Executar assim mesmo**.
3. **+ Nova conta**: cole o endereço do jogo e dê um nome.
4. Repita para as outras contas e escolha **Uma** ou **Grade** no canto
   superior direito.

Botão direito numa conta ou num grupo mostra todas as opções.

**Requisitos:** Windows 10 ou 11. O Idlerx usa o motor do Microsoft Edge
(WebView2), que já vem no Windows 11 e chega pelo Windows Update no Windows
10. Os sites funcionam como no Edge ou no Chrome. A interface está em
português e inglês, seguindo o idioma do Windows.

## Perguntas frequentes

**É de graça mesmo?**
Sim, e sempre vai ser: sem anúncios, sem versão paga, sem cadastro. O
código é aberto (GPL-3.0).

**Minhas contas e senhas ficam onde?**
Só no seu computador. Os logins de cada conta ficam na pasta de perfil
dela, e as senhas e tokens de VPN são criptografados pelo Windows. Nada vai
para servidor nenhum.

**O Idlerx se atualiza sozinho?**
Ele confere se há versão nova logo ao abrir e depois a cada hora, e mostra
um aviso discreto. Ao atualizar, o arquivo novo só é aceito se o código de
verificação (SHA-256) bater com o publicado no GitHub, e o Idlerx reabre
sozinho com as mesmas contas.

**Por que o Windows avisa na primeira vez?**
O SmartScreen avisa sobre programas que ainda não são muito baixados. Cada
versão é gerada direto do código deste repositório, e dá para conferir:
veja [Segurança](#segurança).

**Preciso instalar alguma coisa?**
Não. É um único arquivo. O OpenVPN só é necessário se você usar VPN nos
grupos, e o próprio Idlerx oferece instalar.

## Privacidade

O Idlerx não coleta nem envia dados. As únicas conexões são os **sites que
você abre**, a **verificação de versão** no GitHub (sem nenhum dado seu) e,
nos grupos com VPN, o **servidor da sua VPN** e uma consulta a
`api.ipify.org` **pela própria VPN**, para mostrar o IP que os sites veem.

## Segurança

Cada versão é gerada exclusivamente pelo
[workflow de release](.github/workflows/release.yml) no GitHub Actions, a
partir do código deste repositório. Para conferir a sua cópia:

```powershell
Get-FileHash .\Idlerx.exe -Algorithm SHA256
gh attestation verify .\Idlerx.exe --repo Antxj/Idlerx
```

Compare o SHA-256 com o mostrado na página de [Releases](../../releases); o
segundo comando confere o atestado de origem assinado pelo GitHub (requer o
[GitHub CLI](https://cli.github.com)).

## Uso responsável

O Idlerx organiza sessões de navegador e tem macros que repetem cliques e
teclas que você mesmo definiu; ele não captura credenciais. Muitos jogos
proíbem automação e o uso de várias contas: confira os termos de uso de
cada jogo antes de usar.

## Onde ficam os dados

- Configurações, contas e registro das páginas: `%APPDATA%\Idlerx\`
- Perfis do navegador (logins, cookies, cache) de cada conta:
  `%LOCALAPPDATA%\Idlerx\profiles\`

**Desinstalar:** feche o Idlerx e apague o `Idlerx.exe`. Para remover também
as contas, apague as pastas `%APPDATA%\Idlerx` e `%LOCALAPPDATA%\Idlerx`.

## Licença

Gratuito e de código aberto, sob a [GPL-3.0-or-later](LICENSE). Componentes
de terceiros e suas licenças estão em
[THIRD-PARTY-LICENSES.txt](THIRD-PARTY-LICENSES.txt).
