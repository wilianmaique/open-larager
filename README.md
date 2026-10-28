# Open Larager

Gerenciador de ambientes de desenvolvimento para **Windows 11 x64**, feito com Tauri 2, Rust, React e TypeScript. Sem servidores pré-instalados e sem telemetria. A interface inclui português brasileiro, inglês, espanhol, chinês simplificado e hindi, com detecção do idioma do sistema, temas e suporte a movimento reduzido.

PHP, Node.js com npm/npx, Composer, Apache, MySQL, PostgreSQL, Nginx, Redis, Mailpit e o gateway HTTPS Caddy são baixados individualmente, conforme a escolha do usuário. Cada download é verificado pelo SHA-256 fixado no catálogo. As distribuições usam ZIP; Composer usa o PHAR oficial. Nenhum instalador desses componentes é executado e nenhum serviço do Windows é registrado.

## Executar e compilar

Para desenvolver: Node.js 22.12+, pnpm 12.15, Rust MSVC 1.90+ e as ferramentas C++ do Visual Studio. Veja os [pré-requisitos oficiais do Tauri](https://v2.tauri.app/start/prerequisites/).

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

## Publicar uma versão

O workflow **Windows verification** verifica branches e pull requests. O **Windows release** roda ao enviar uma tag `vX.Y.Z`, reaproveita essas verificações e prepara um **rascunho** no GitHub Releases com instalador `.exe`, ZIP portátil e SHA-256. Para gerar somente o instalador localmente, use `pnpm desktop:bundle`.

Para uma nova versão, execute `pnpm release:version 0.2.0`, revise e faça commit dos cinco arquivos de versão, envie o commit e depois a tag `v0.2.0`. Aguarde o workflow, revise os downloads em **Releases** e clique em **Publish release**. Tags como `v0.2.0-rc.1` são marcadas como pré-release. Veja o [passo a passo de publicação](docs/releasing.md), incluindo assinatura de código e validações pendentes.

## Primeiro projeto com HTTPS

1. Instale a versão desejada do PHP, o Nginx e o Caddy pelo painel de serviços.
2. Em **Domínios e HTTPS**, adicione `meu-site.test`, selecione **Nginx + HTTPS** e escolha a pasta pública do projeto. Para Laravel, selecione `public`.
3. Inicie PHP, Nginx e Caddy. A opção **Caddy direto** atende o site sem Nginx; domínios existentes mantêm essa opção.
4. Aplique os hosts. O Windows solicitará elevação somente para atualizar o bloco do app nesse arquivo.
5. Confie na CA local pelo botão correspondente. Ela é adicionada ao armazenamento de certificados do usuário atual, com confirmação do Windows quando necessária.
6. Abra `https://meu-site.test`. Portas HTTP, HTTPS e de administração do gateway também são configuráveis.

Também é possível servir arquivos estáticos ou encaminhar um domínio para uma porta local de outra aplicação. A CA local é exclusiva do ambiente, gera e renova os certificados via Caddy e pode ser removida pelo painel. Ela nunca solicita certificados públicos para esses domínios.

## Extensões do PHP

No painel do PHP, abra **Extensões**, busque a extensão e clique em **Ativar** ou **Instalar e ativar**. O app identifica a versão real do PHP, x64, TS/NTS e compilador. Extensões incluídas na distribuição usam suas próprias DLLs; Redis, Xdebug, APCu e igbinary usam builds PECL oficiais com SHA-256 fixado. Sem build compatível, a instalação fica indisponível. Extensões integradas ao executável aparecem sem ação de remoção.

O app preserva as demais opções do `php.ini`, ativa dependências incluídas quando necessárias e testa o carregamento antes de aplicar. PHP em execução reinicia automaticamente; se o reinício falhar, o app tenta restaurar a configuração e iniciar o estado anterior. PHP parado continua parado. Nginx, Apache e Caddy mantêm seus listeners. Cada versão do PHP mantém suas próprias escolhas, inclusive ao trocar a versão ativa; desativar preserva as DLLs em cache. O Composer e o comando `php` registrado no PATH usam automaticamente a configuração gerenciada.

## Comandos no terminal

Ao instalar um pacote, **Adicionar a versão ativa ao meu PATH** é opcional e vem desmarcado. O painel do serviço também permite registrar, remover ou reparar essa entrada depois. O app altera apenas o PATH do usuário Windows, sem elevação, e informa a versão e a pasta selecionadas.

O PATH acompanha a **Versão ativa**. Instalar outra versão não muda a ativa; ao selecionar e salvar outra versão instalada, o app substitui a entrada que gerencia. Ao remover a versão ativa, o PATH acompanha a versão restante indicada na confirmação; sem versões restantes, a entrada é removida. Entradas externas, inclusive uma entrada idêntica que já existia antes do registro, são preservadas. Se outra instalação tiver prioridade no PATH do Windows, o painel mostra o caminho encontrado.

Feche e reabra o aplicativo de terminal, incluindo Windows Terminal ou VS Code, após registrar ou remover o PATH. O comando gerenciado `php` fica em `config/cli/php/php.cmd` e carrega o `php.ini` e as extensões da versão ativa, também nos processos PHP que inicia. Trocas de versão e alterações de extensões valem no próximo comando, inclusive em terminais já abertos com essa pasta no PATH. Registros antigos do PHP são atualizados ao abrir o app; após essa atualização, reabra o terminal uma vez. O registro não altera `PHPRC` globalmente. Mover o ambiente por **Preferências → Alterar pasta** atualiza as entradas gerenciadas. Para movê-lo manualmente ou apagá-lo fora do app, remova essas entradas antes pelo painel.

Node.js e Composer aparecem como ferramentas de terminal, com estado **Pronto** e seleção de versão. Não possuem processo de fundo, portas ou inicialização automática. O npm e o npx vêm da mesma distribuição do Node.js; o painel mostra a versão incluída. O catálogo oferece duas linhas LTS e a versão corrente do Node.js. Atualizações passam pelo catálogo; evite `npm install -g npm` ou `composer self-update` nas distribuições gerenciadas.

O PATH do Node.js registra os comandos gerenciados, a pasta do runtime ativo e seus comandos globais. Os comandos `npm` e `npx` usam esse `node.exe`, com prefixo global e cache em `data/nodejs/<versão>/` e configuração em `config/nodejs/<versão>/npmrc`. Pacotes globais permanecem ao remover os binários e voltam a ficar disponíveis ao reinstalar e ativar a mesma versão.

O Composer pode ser baixado separadamente; sem PHP ativo, informa **Requer dependência**. Para registrá-lo no PATH, instale e selecione o PHP. Seu comando usa explicitamente o PHP ativo, o `php.ini` gerenciado e o mesmo PHP nos scripts `@php`, sem exigir registro separado do PHP. Trocar PHP ou Composer atualiza o comando na mesma transação do estado e PATH. Desative o registro do Composer antes de remover o último PHP. Estado global e cache ficam em `data/composer/<versão>/`, preservados na remoção.

Cada site Nginx recebe `config/sites/<id>/nginx.conf`, com domínio, pasta pública, FastCGI ou proxy e logs próprios em `logs/sites/<id>/`. No botão **Configuração do site** da linha do domínio, edite regras de rotas, cabeçalhos, cache e limites de upload, visualize o arquivo completo ou restaure os padrões. O app gerencia listeners em loopback, caminhos e conexão com PHP; esses campos são alterados pelos formulários.

Pare o Nginx antes de alterar seus sites. Salvar regras exige Nginx instalado, valida todos os sites com `nginx -t` e mantém uma cópia `.previous`; inicie novamente para aplicar. Regras personalizadas permanecem ao trocar portas ou versões. Remover um domínio desativa sua inclusão, preservando os arquivos de configuração, logs e projeto. A configuração global continua disponível no painel do serviço.

## Organização dos arquivos

Por padrão, o ambiente fica em `%LOCALAPPDATA%/dev.openlarager.desktop`. Crie um arquivo vazio chamado `larager.portable` ao lado do executável **antes de abri-lo** para usar `larager-data` nesse mesmo diretório.

Em **Preferências → Alterar pasta**, escolha uma pasta local vazia e confirme a origem e o destino. Pare os serviços e feche terminais e ferramentas externas antes. O app copia e verifica os arquivos com SHA-256, ajusta os caminhos das configurações e dos projetos internos e atualiza seu PATH gerenciado. Bancos e credenciais são preservados, sem trocar versões. Os serviços continuam parados; reabra os terminais depois.

Durante a mudança, a barra acompanha os bytes processados na cópia, verificação e limpeza, com volume copiado e contagem de arquivos. O tempo restante é estimado pela velocidade medida; aparece após a amostra inicial e pode variar. A leitura inicial do tamanho não exibe percentual, e 100% só aparece após a conclusão das pendências.

A pasta antiga é limpa após a cópia verificada. Arquivos alterados, bloqueados ou adicionados por outras ferramentas permanecem protegidos; o painel mostra pendências e permite tentar novamente, inclusive após reabrir o app. Links e junctions são recusados. A escolha fica em `%APPDATA%/dev.openlarager.desktop/environment-location.json`, ou `larager-environment.json` ao lado do executável portátil. Se o destino estiver desconectado, o app exige reconectá-lo e não cria outro ambiente vazio. Uma interrupção abrupta durante a cópia, antes de ativar o destino, conserva a origem e pode deixar uma cópia parcial no destino.

Falhas de arquivo mostram a operação, o caminho completo e o código do Windows. Nos erros de bloqueio, o app consulta os processos que usam o arquivo e exibe nome, PID e executável quando disponíveis, sem encerrá-los. A consulta pode não identificar processos protegidos, bloqueios temporários ou diretórios; essa limitação aparece no diagnóstico.

O perfil da interface WebView2 fica separado em `%LOCALAPPDATA%/dev.openlarager.desktop.webview`, ou `larager-webview/` ao lado do executável portátil. Ao abrir no modo normal, um perfil antigo `EBWebView` na raiz padrão é movido antes de iniciar a interface; se já existir um perfil novo, o antigo fica arquivado como `legacy-<UUID>`, sem substituí-lo. Se outro processo impedir essa mudança, o perfil antigo é preservado. Assim, o próprio navegador da interface deixa de bloquear a movimentação do ambiente.

```text
packages/<serviço>/<versão>/   Binários da distribuição
packages/php-extensions/      DLLs PECL isoladas por versão do PHP e SHA-256
config/<serviço>/<versão>/     Configurações e uma cópia anterior
data/<serviço>/<versão>/       Dados persistentes, separados por versão
data/tls/                     Autoridade certificadora e certificados locais
logs/<serviço>/<versão>/       Logs dos processos e serviços
config/sites/<id>/            Configuração Nginx gerada por domínio
config/cli/<ferramenta>/       Comandos de terminal gerenciados
logs/sites/<id>/              Logs de acesso e erro por domínio
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
cargo test --manifest-path src-tauri/Cargo.toml --test nginx_sites -- --ignored --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test toolchains -- --ignored --nocapture
```

Eles não escrevem nos hosts nem alteram certificados confiáveis do Windows. A suíte verifica consultas autenticadas em MySQL e PostgreSQL, Redis, PHP por FastCGI, Mailpit, TLS validado pela CA do ambiente, reinício, liberação de portas e preservação dos dados.

O teste `toolchains` baixa duas versões de Node.js e PHP e o Composer; verifica npm/npx, configuração e pacote global local, scripts Composer com o PHP selecionado e preservação dos dados em uma pasta temporária com Unicode. Ele não altera o PATH real; as transações de PATH são testadas com ambiente simulado e chave de registro isolada.

Depois do build, `pnpm test:desktop` verifica o executável real com WebView2, IPC, uma instalação portátil temporária do Mailpit, o editor local e o encerramento da janela. O diretório temporário é removido ao terminar; hosts e certificados confiáveis não são alterados.

Execute esse teste numa sessão padrão do Windows 11 x64. O runner hospedado pelo GitHub usa administrador com UAC desabilitado, incompatível com a proteção do app contra serviços elevados. Antes de publicar um rascunho, teste seu executável baixado com `pnpm test:desktop --smoke <caminho-do-open-larager.exe>` e registre o resultado nas notas do release.

Veja [arquitetura e limites de segurança](docs/architecture.md) e [publicação de releases](docs/releasing.md).
