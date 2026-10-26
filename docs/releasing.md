# Publicar uma release Windows

1. Execute a suíte de interface, testes Rust, Clippy e os testes reais documentados no README.
2. Em uma VM Windows 11 x64 limpa, verifique WebView2, detecção de runtime C++, permissões de diretórios, portas ocupadas e caminhos com espaços e caracteres acentuados.
3. Verifique os fluxos interativos de UAC dos hosts e confirmação do certificado, incluindo cancelamento, remoção de domínio, remoção da CA e preservação de entradas alheias. Os testes automáticos não alteram a confiança nem os hosts do computador de desenvolvimento.
4. Teste encerramento normal, encerramento forçado, falta de espaço, download cancelado e nova abertura após uma instalação interrompida.
5. Gere `pnpm desktop:build`. Distribua `src-tauri/target/release/open-larager.exe`. Um instalador do gerenciador é opcional: `pnpm tauri build --bundles nsis`; ele também não contém servidores.
6. Antes de publicar para terceiros, assine o executável e eventual instalador com um certificado de assinatura de código válido, inclua timestamp e verifique a assinatura. Credenciais de assinatura pertencem ao cofre de CI, nunca ao repositório. Um executável de desenvolvimento sem assinatura não deve ser apresentado como uma release assinada ou homologada.
7. Publique SHA-256 do executável e notas com versões do catálogo, alterações de formato de dados e requisitos. Confira as licenças das distribuições: os serviços não são redistribuídos no binário, mas suas licenças continuam aplicáveis ao download e uso.

## Atualizar distribuições

`pnpm catalog:refresh` consulta os distribuidores e gera o catálogo completo. Para pacotes sem SHA-256 publicado, o script baixa o arquivo oficial para calcular o hash sem armazenar o ZIP no repositório. Uma falha mantém o catálogo anterior.

Revise o diff, as URLs, os executáveis e a estrutura interna dos arquivos. Uma versão nova pode alterar requisitos de runtime ou parâmetros de inicialização. Verifique assinaturas dos distribuidores quando fornecidas e execute os testes reais antes de publicar. Não substitua um hash apenas para contornar uma falha de integridade.
