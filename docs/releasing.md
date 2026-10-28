# Publicar uma release Windows

## Fluxo no GitHub

- **`.github/workflows/ci.yml` — Windows verification**: verifica branches e pull requests, sem criar releases. Também é reutilizado pelo workflow de release.
- **`.github/workflows/release.yml` — Windows release**: ao enviar uma tag `v*`, verifica a versão, executa build, Playwright, testes Rust, Clippy e smoke test do desktop, gera os downloads e cria um **rascunho** com notas automáticas. A publicação fica a seu critério.

O job de compilação tem acesso de leitura ao repositório; somente o job que anexa os downloads recebe `contents: write`. O `GITHUB_TOKEN` é fornecido pelo GitHub: não é necessário criar um PAT. O workflow não inclui assinatura Authenticode; ela continua como requisito pendente antes da distribuição conforme a lista abaixo.

### Preparar e enviar a tag

1. Na branch que será publicada, atualize a versão. O comando sincroniza `package.json`, `package-lock.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` e a entrada do app em `src-tauri/Cargo.lock`, sem atualizar dependências:

   ```powershell
   pnpm release:version 0.2.0
   pnpm release:check v0.2.0
   ```

2. Revise o diff, faça commit dos cinco arquivos e envie a branch. Use uma tag anotada no commit exato que quer distribuir:

   ```powershell
   git add package.json package-lock.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
   git commit -m "chore: release 0.2.0"
   git push origin HEAD
   git tag -a v0.2.0 -m "Open Larager 0.2.0"
   git push origin v0.2.0
   ```

   Para a primeira versão atual, `0.1.0`, basta conferir com `pnpm release:check v0.1.0` e enviar a tag correspondente depois de subir os workflows. Para testes públicos, use versões como `0.2.0-rc.1` ou `0.2.0-beta.1`; o rascunho será marcado como pré-release. A tag deve corresponder exatamente à versão dos cinco arquivos; divergências interrompem o workflow antes do build.

3. Em **Actions → Windows release**, aguarde a conclusão e abra o link do rascunho no resumo da execução. Os mesmos arquivos também ficam disponíveis como artifact por sete dias, inclusive se a etapa de criação do rascunho falhar.
4. Em **Releases**, revise as notas e os arquivos, cumpra as validações e a assinatura abaixo e clique em **Publish release**. A partir daí, compartilhe a página do release para o público baixar. Antes de publicar, os assets do rascunho ficam restritos a quem tem acesso de escrita.

Os downloads são anexos do release, separados do código e dos artifacts temporários do Actions:

| Arquivo                                       | Uso                                                                    |
| --------------------------------------------- | ---------------------------------------------------------------------- |
| `Open-Larager_0.2.0_windows-x64-setup.exe`    | Instalador NSIS para o usuário atual.                                  |
| `Open-Larager_0.2.0_windows-x64-portable.zip` | Executável e marcador `larager.portable`; extrair tudo antes de abrir. |
| `SHA256SUMS.txt`                              | SHA-256 dos dois arquivos acima.                                       |

O instalador e o ZIP contêm somente o gerenciador. WebView2 é pré-requisito; PHP, bancos e servidores continuam sendo baixados individualmente no app. Os arquivos **Source code** que o GitHub adiciona são o código-fonte, não o instalador.

Para repetir uma tentativa, use **Re-run jobs** na execução original ou **Actions → Windows release → Run workflow**, selecione `main` e informe uma **tag já existente**. O workflow compila o conteúdo dessa tag e só substitui anexos enquanto o release for rascunho, preservando notas editadas. Depois de publicado, use uma nova versão e uma nova tag; não mova tags já distribuídas. A opção manual aparece depois que `release.yml` estiver na branch padrão.

Para preparar arquivos localmente: execute `pnpm desktop:bundle` e `pwsh -NoProfile -File scripts/package-release.ps1`. Os downloads ficam em `release-assets/`, ignorado pelo Git. A pasta de saída deve estar ausente para não misturar builds; preserve ou mova arquivos anteriores antes de repetir. Se assinar os arquivos depois da geração, regenere os hashes de **todos** os downloads e atualize as notas antes de anexá-los. Não execute novamente o workflow sobre um rascunho que já recebeu arquivos assinados manualmente.

Referências: [releases e rascunhos no GitHub](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository), [instalador Windows do Tauri](https://v2.tauri.app/distribute/windows-installer/) e [assinatura Windows](https://v2.tauri.app/distribute/sign/windows/).

## Validação antes de publicar

1. Execute a suíte de interface, testes Rust, Clippy e os testes reais documentados no README.
2. Em uma VM Windows 11 x64 limpa, verifique WebView2, detecção de runtime C++, permissões de diretórios, portas ocupadas e caminhos com espaços e caracteres acentuados.
   Validação interativa pendente do PATH no executável empacotado: opção desmarcada inicialmente, duas versões do PHP, seleção e remoção da ativa, entrada preexistente, conflito com PATH da máquina, nova janela de terminal e recuperação após interrupção. Os testes de registro usam uma chave isolada e não alteram o PATH real do desenvolvedor.
   Para Node.js/npm/npx e Composer, validar também no executável empacotado: terminais PowerShell e cmd, conflitos por comando, troca de runtime, reinstalação com pacotes globais preservados e dependência do PHP. O teste real automatizado usa launchers temporários e não registra o PATH do desenvolvedor.
   Para o PHP no terminal, executar `cargo test --test toolchains php_cli_uses_active_configuration_and_extensions_in_both_shells -- --ignored --nocapture` em `src-tauri`. O teste usa duas versões fixadas em ambiente temporário, PowerShell e cmd, PDO PostgreSQL, alteração de extensões, argumentos, subprocessos e códigos de saída. Validar interativamente no executável empacotado a atualização de registros antigos e a reabertura única do terminal; não é necessário configurar `PHPRC` manualmente.
   Validação interativa pendente no executável empacotado: criar sites Nginx PHP, estático e proxy; editar, validar, restaurar regras e conferir a prévia completa e o aviso de mudanças não salvas nos cinco idiomas.
   Para **Alterar pasta**, validar no executável empacotado e em Windows 11 limpo: seletor e cancelamento nos cinco idiomas, movimentação entre volumes, pouco espaço, caminhos Unicode com aliases indisponíveis, configurações personalizadas, bancos reais após a mudança, PATH em novos terminais, arquivo bloqueado, drive desconectado e interrupções antes/depois da ativação. A suíte automatizada cobre cópia e hashes de dados temporários, limpeza, retomada, junctions e PATH simulado; não certifica esses cenários de release.
   O diagnóstico de bloqueios tem teste real com arquivo temporário aberto por outro processo Windows. Ainda validar interativamente os erros 32/33 com antivírus e processos protegidos, diretórios em uso, migração do perfil WebView2 legado em instalações existentes e as mensagens nos cinco idiomas. `pnpm test:desktop --smoke` verifica janela empacotada, IPC, perfil portátil separado e encerramento sem baixar servidores.
3. Verifique os fluxos interativos de UAC dos hosts e confirmação do certificado, incluindo cancelamento, remoção de domínio, remoção da CA e preservação de entradas alheias. Os testes automáticos não alteram a confiança nem os hosts do computador de desenvolvimento.
   Para extensões PHP, executar `cargo test --test php_extensions -- --ignored --nocapture` em `src-tauri`. Validação interativa pendente em Windows 11 limpo e no executável empacotado: seleção e busca nos cinco idiomas, DLL incompatível, download cancelado, falta de runtime C++, interrupção durante recarga, falha de restauração e requisições FastCGI durante o reinício. O teste automatizado usa ambientes temporários com acentos e não altera instalações existentes.
4. Teste encerramento normal, encerramento forçado, falta de espaço, download cancelado e nova abertura após uma instalação interrompida.
   Validação interativa pendente: abrir normalmente e como administrador, confirmar a reabertura com token filtrado e iniciar PostgreSQL; testar UAC aceito e cancelado para hosts. Contas sem token filtrado devem falhar sem iniciar serviços elevados. Conferir logs UTF-8 e na página ANSI do Windows, inclusive com acentos. Logs legados sem indicação de codificação usam a página ANSI local quando não são UTF-8 válidos.
5. Teste os arquivos gerados pelo workflow. Para builds locais, `pnpm desktop:build` gera `src-tauri/target/release/open-larager.exe`; `pnpm desktop:bundle` também gera o instalador NSIS. Verifique instalação, atualização e desinstalação do gerenciador, preservando o ambiente do usuário. Esses cenários do instalador ainda exigem validação interativa em Windows 11 limpo.
   O bundler avisa que o Tauri ainda não fornece suas mensagens próprias do NSIS em Hindi. Revisar essas mensagens e fornecer `customLanguageFiles` antes de declarar o instalador integralmente traduzido nos cinco idiomas.
6. Antes de publicar para terceiros, assine o executável e eventual instalador com um certificado de assinatura de código válido, inclua timestamp e verifique a assinatura. Credenciais de assinatura pertencem ao cofre de CI, nunca ao repositório. Um executável de desenvolvimento sem assinatura não deve ser apresentado como uma release assinada ou homologada.
7. Publique SHA-256 do executável e notas com versões do catálogo, alterações de formato de dados e requisitos. Confira as licenças das distribuições: os serviços não são redistribuídos no binário, mas suas licenças continuam aplicáveis ao download e uso.

## Atualizar distribuições

`pnpm catalog:refresh` consulta os distribuidores e gera o catálogo completo. Para pacotes sem SHA-256 publicado, o script baixa o arquivo oficial para calcular o hash sem armazenar o ZIP no repositório. Uma falha mantém o catálogo anterior.

`node scripts/refresh-toolchains.mjs` atualiza somente Node.js e Composer, preservando os demais pacotes. Ambos usam checksums publicados; o npm acompanha a versão declarada pela distribuição do Node.js.

`pnpm catalog:extensions` atualiza apenas os builds PECL do catálogo de extensões para as linhas PHP já selecionadas. Baixa os ZIPs oficiais para fixar SHA-256, sem incluí-los no repositório. A ausência de um build desabilita essa combinação, sem escolher outra ABI. Revise o catálogo e execute o teste real após cada atualização.

Revise o diff, as URLs, os executáveis e a estrutura interna dos arquivos. Uma versão nova pode alterar requisitos de runtime ou parâmetros de inicialização. Verifique assinaturas dos distribuidores quando fornecidas e execute os testes reais antes de publicar. Não substitua um hash apenas para contornar uma falha de integridade.
