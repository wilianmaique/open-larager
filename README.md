# Open Larager

Gerenciador de ambientes de desenvolvimento para **Windows 11 x64**, feito com Tauri 2, Rust, React e TypeScript. Sem servidores pré-instalados e sem telemetria. A interface inclui português brasileiro, inglês, espanhol, chinês simplificado e hindi, com detecção do idioma do sistema, temas e suporte a movimento reduzido.

PHP, Apache, MySQL, PostgreSQL, Nginx, Redis, Mailpit e o gateway HTTPS Caddy são baixados individualmente, conforme a escolha do usuário. Cada download é verificado pelo SHA-256 fixado no catálogo antes de ser extraído. Instalações utilizam arquivos ZIP, sem registrar serviços do Windows ou executar instaladores desses componentes.

## Executar e compilar

Para desenvolver: Node.js 22.12+, pnpm 11.21, Rust MSVC 1.90+ e as ferramentas C++ do Visual Studio. Veja os [pré-requisitos oficiais do Tauri](https://v2.tauri.app/start/prerequisites/).

```powershell
pnpm install --frozen-lockfile
pnpm desktop:dev
```

Para gerar o executável independente:

```powershell
pnpm desktop:build
```

Resultado: `src-tauri/target/release/open-larager.exe`. O executável contém a interface e o motor; não contém PHP, bancos, servidores, Node.js ou Rust. O WebView2 deve estar disponível no Windows. Algumas distribuições, especialmente Apache e PHP, precisam do [Microsoft Visual C++ Redistributable x64](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist). O app não instala esse pré-requisito silenciosamente.

`pnpm dev` abre somente a prévia da interface no navegador. As operações do sistema funcionam no executável Tauri.

## Primeiro projeto com HTTPS

1. Instale a versão desejada do PHP e o Caddy pelo painel de serviços.
2. Inicie os dois serviços. Apache e Nginx são opções independentes; o gateway também serve PHP diretamente.
3. Em **Domínios e HTTPS**, adicione `meu-site.test` e escolha a pasta pública do projeto. Para Laravel, selecione `public`.
4. Aplique os hosts. O Windows solicitará elevação somente para atualizar o bloco do app nesse arquivo.
5. Confie na CA local pelo botão correspondente. Ela é adicionada ao armazenamento de certificados do usuário atual, com confirmação do Windows quando necessária.
6. Abra `https://meu-site.test`. Portas HTTP, HTTPS e de administração do gateway também são configuráveis.

Também é possível servir arquivos estáticos ou encaminhar um domínio para uma porta local de outra aplicação. A CA local é exclusiva do ambiente, gera e renova os certificados via Caddy e pode ser removida pelo painel. Ela nunca solicita certificados públicos para esses domínios.

## Organização dos arquivos

Por padrão, o ambiente fica em `%LOCALAPPDATA%/dev.openlarager.desktop`. Crie um arquivo vazio chamado `larager.portable` ao lado do executável **antes de abri-lo** para usar `larager-data` nesse mesmo diretório.

```text
packages/<serviço>/<versão>/   Binários da distribuição
config/<serviço>/<versão>/     Configurações e uma cópia anterior
data/<serviço>/<versão>/       Dados persistentes, separados por versão
data/tls/                     Autoridade certificadora e certificados locais
logs/<serviço>/<versão>/       Logs dos processos e serviços
projects/                     Projetos do usuário
secrets/                      Credenciais cifradas pelo Windows DPAPI
backups/                      Estado anterior e recuperação de configurações
.staging/                     Downloads e extração temporários
```

Desinstalar um pacote remove apenas os binários. Bancos, logs e configurações permanecem. Trocar uma versão de banco **não migra os dados**: faça dump/restore com as ferramentas do banco. Credenciais DPAPI permanecem vinculadas à conta e ao computador Windows; copiar uma pasta portátil para outro computador não transfere essas credenciais.

O app verifica conflitos de portas, inicia os bancos com senhas exclusivas e utiliza encerramento próprio dos serviços quando disponível. Os processos ficam em Windows Job Objects: uma interrupção do gerenciador encerra seus descendentes. Ao fechar normalmente, o app solicita a parada dos serviços primeiro.

## Verificação

```powershell
pnpm build
pnpm exec playwright install chromium
pnpm test:e2e
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

No CI, `PLAYWRIGHT_PREVIEW=1` faz o Playwright testar a interface compilada em `dist/`, pela porta 1421. O editor Monaco deve continuar sendo carregado apenas ao abrir a configuração de um serviço.

Os testes reais são separados porque baixam aproximadamente 1 GB de distribuições e inicializam bancos em um diretório temporário:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test portable_lifecycle -- --ignored --nocapture --test-threads=1
```

Eles não escrevem nos hosts nem alteram certificados confiáveis do Windows. A suíte verifica consultas autenticadas em MySQL e PostgreSQL, Redis, PHP por FastCGI, Mailpit, TLS validado pela CA do ambiente, reinício, liberação de portas e preservação dos dados.

Depois do build, `pnpm test:desktop` verifica o executável real com WebView2, IPC, uma instalação portátil temporária do Mailpit, o editor local e o encerramento da janela. O diretório temporário é removido ao terminar; hosts e certificados confiáveis não são alterados.

Veja [arquitetura e limites de segurança](docs/architecture.md) e [publicação de releases](docs/releasing.md).
