## Downloads para Windows 11 x64

- **`windows-x64-setup.exe`**: instalador do Open Larager para o usuário atual.
- **`windows-x64-portable.zip`**: extraia tudo em uma pasta gravável e abra `open-larager.exe`. O arquivo `larager.portable` já está incluído.
- **`SHA256SUMS.txt`**: hashes SHA-256 dos dois downloads. Compare com `Get-FileHash <arquivo> -Algorithm SHA256` no PowerShell.

WebView2 Runtime deve estar instalado. Alguns pacotes opcionais exigem o Microsoft Visual C++ Redistributable x64. PHP, bancos e servidores são baixados somente quando selecionados no app.

**Assinatura Authenticode ainda não configurada neste workflow.** Os artefatos gerados não são assinados; SHA-256 verifica integridade e não substitui assinatura de código. O Windows pode exibir avisos de segurança.

<!-- Antes de publicar: revise docs/releasing.md, registre os testes manuais e de serviços reais realizados, as versões do catálogo e eventuais mudanças de dados. Revise a informação de assinatura se substituir os arquivos por versões assinadas e regenere SHA256SUMS.txt. -->
