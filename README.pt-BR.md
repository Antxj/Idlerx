<p align="center">
  <img src="docs/logo.png" width="112" alt="Ícone do Idlerx">
</p>

<h1 align="center">Idlerx</h1>

<p align="center"><a href="README.md">English</a> | <b>Português</b></p>

<p align="center">
  <a href="https://github.com/Antxj/Idlerx/releases/latest"><img src="https://img.shields.io/github/v/release/Antxj/Idlerx?label=Baixar&style=for-the-badge&color=14b8a6" alt="Baixar a versão mais recente"></a>
</p>

Aplicativo para Windows que abre **várias contas do mesmo site ou jogo ao
mesmo tempo**, cada uma **isolada de verdade** (login, cookies, cache e
armazenamento próprios), organizadas numa única janela: uma por vez ou todas
lado a lado numa grade. Feito para quem joga jogos idle de navegador com mais
de uma conta.

**Gratuito e de código aberto** (GPL-3.0). Escrito em **Rust**: um único
executável, sem instalador.

A interface está em português do Brasil e em inglês, seguindo o idioma do
Windows (dá para fixar em **Configurações** ⚙ › Idioma).

## Recursos

- **Isolamento de verdade**: cada conta tem a própria pasta de perfil do
  navegador. Dá para logar em duas contas do mesmo jogo ao mesmo tempo, sem
  aba anônima, e uma nunca derruba a outra
- **Um processo por conta**: cada conta roda no seu próprio processo do
  navegador; se uma travar, as outras seguem funcionando
- **Uma por vez ou em grade**: a grade se ajusta sozinha ao tamanho da
  janela (ou escolha o número de colunas). **Arraste uma conta da lista até
  um quadro** da grade para colocá-la ali
- **Modo foco** (F11): some com a lista e a barra, fica só o jogo. F11 ou Esc
  traz tudo de volta
- **CPU e memória** de cada conta (na lista e no quadro da grade) e o
  **total** (rodapé da lista; passe o mouse para ver contas × interface)
- **Áudio e zoom por conta**: silencie uma, dê zoom em outra (também com
  Ctrl+roda do mouse dentro da página). O ícone 🔊 mostra quem está tocando som
- **Barra de endereço** com voltar, avançar e recarregar: é só colar a URL de
  qualquer jogo
- **Favoritar, renomear, reordenar** (arrastando na lista)
- **Pausar** uma conta fecha a página e libera a memória; ao abrir de novo
  ela continua logada
- **Limpar dados** (sair da conta) recomeça a conta do zero, sem removê-la
- **Reabre as contas** que estavam abertas, no último endereço visitado
- **Atualizações automáticas** pelo GitHub (detalhes em
  [Atualizações](#atualizações))
- **Instância única**: abrir o executável de novo só traz a janela para a frente

### Economia de recursos

- Páginas fora da tela (outra conta selecionada, menu por cima) **param de
  desenhar** e o WebView2 é instruído a **usar menos memória**, mas o jogo
  continua rodando
- "Manter os jogos ativos em segundo plano" (ligado por padrão) impede o
  navegador de desacelerar os timers de páginas escondidas: jogos idle
  continuam progredindo. Desligue para economizar ainda mais CPU
- A interface só se redesenha quando algo muda (ou 1×/s para os números)
- **Pausar** contas que você não está usando libera toda a memória delas

## Requisitos

- **Windows 10 ou 11**
- **Microsoft Edge WebView2 Runtime**: já vem no Windows 11 e é instalado
  pelo Windows Update no Windows 10. Se faltar, o Idlerx mostra um aviso com
  o link oficial da Microsoft

O Idlerx usa o motor do Microsoft Edge (Chromium), então os sites se
comportam como no Edge ou no Chrome.

## Como usar

1. Baixe o `Idlerx.exe` na página de [Releases](../../releases)
2. Abra. Na primeira vez, o SmartScreen pode avisar sobre "aplicativo não
   reconhecido": clique em **Mais informações** › **Executar assim mesmo**
3. **+ Nova conta**: cole o endereço do jogo e dê um nome (ex.: "Char
   principal")
4. Repita para as outras contas e escolha **▣ Uma** ou **⊞ Grade** no canto
   superior direito

Clique com o botão direito numa conta para mais opções (editar, silenciar,
tirar da grade, limpar dados, remover).

## Para desenvolvedores

Requisitos: [Rust](https://rustup.rs) com o toolchain **MSVC** (o
`rust/rust-toolchain.toml` já seleciona) e o Visual Studio Build Tools. O
toolchain GNU não serve: com ele o loader do WebView2 vira uma DLL separada.

```powershell
cd rust
cargo test
cargo run
.\build-release.ps1      # testes + release sem caminhos locais + verificações
```

Estrutura:

- [`main.rs`](rust/src/main.rs): inicialização e janela
- [`app.rs`](rust/src/app.rs): interface (egui): lista, barra, grade, diálogos
- [`panels.rs`](rust/src/panels.rs): uma conta aberta (WebView2 via
  [webview2-com](https://github.com/wravery/webview2-rs), criado de forma
  assíncrona): posição, mudo, zoom, navegação
- [`config.rs`](rust/src/config.rs): contas e configurações (JSON)
- [`metrics.rs`](rust/src/metrics.rs): CPU e memória por árvore de processos
- [`i18n.rs`](rust/src/i18n.rs): idioma e as macros `tr!`/`trf!`
- [`update.rs`](rust/src/update.rs): verificação e instalação de versões novas
- [`single.rs`](rust/src/single.rs): instância única

Variáveis úteis para testes:

| Variável | Efeito |
|---|---|
| `IDLERX_DATA` | guarda configurações e perfis em outra pasta (não mexe nas contas reais) |
| `IDLERX_INSTANCE` | separa uma instância de teste do app de uso diário |
| `IDLERX_LANGUAGE` | força `pt` ou `en` |
| `IDLERX_UPDATE_URL` | consulta outro endereço em vez da API do GitHub |

## Onde ficam os dados

- Configurações e lista de contas: `%APPDATA%\Idlerx\config.json`
- Perfis do navegador (cookies, cache, logins) de cada conta:
  `%LOCALAPPDATA%\Idlerx\profiles\`

Os dados das contas **nunca saem do computador**.

## Atualizações

Não há servidor próprio: o app consulta
`api.github.com/repos/Antxj/Idlerx/releases/latest` 30 segundos depois de
abrir e depois uma vez por dia (dá para desligar em **Configurações**). A
requisição não envia nenhum dado do usuário.

Quando há versão nova, aparece um link discreto na barra superior. Em
**Atualizar agora**:

1. o `Idlerx.exe` da release é baixado e só é aceito se tiver exatamente o
   SHA-256 que o GitHub publica para o arquivo;
2. se o executável atual for assinado digitalmente, o novo precisa ter
   assinatura válida **do mesmo editor**;
3. o executável atual vira `Idlerx.exe.antigo` (apagado na próxima
   inicialização) e o novo toma o lugar;
4. o app fecha e a versão nova abre sozinha, com as mesmas contas.

## Privacidade

O Idlerx não coleta nem envia dados. As únicas conexões são os **sites que
você abre** e a **verificação de versões** no GitHub (descrita acima).

## Uso responsável

O Idlerx organiza sessões de navegador; ele **não automatiza ações** nos
jogos nem captura credenciais. Respeite os termos de uso de cada jogo,
inclusive quanto ao uso de várias contas.

## Desinstalar

Feche o app e apague o `Idlerx.exe`. Para remover também as contas e os
perfis, apague as pastas `%APPDATA%\Idlerx` e `%LOCALAPPDATA%\Idlerx`.

## Conferindo o download

Cada release é gerada exclusivamente pelo
[workflow de release](.github/workflows/release.yml) no GitHub Actions, a
partir do código deste repositório. Compare o SHA-256 da sua cópia com o
mostrado na página de [Releases](../../releases):

```powershell
Get-FileHash .\Idlerx.exe -Algorithm SHA256
```

E confira o atestado de origem (assinado pelo GitHub) com o
[GitHub CLI](https://cli.github.com):

```powershell
gh attestation verify .\Idlerx.exe --repo Antxj/Idlerx
```

## Licença

[GPL-3.0-or-later](LICENSE). Componentes de terceiros e suas licenças estão
em [THIRD-PARTY-LICENSES.txt](THIRD-PARTY-LICENSES.txt).
