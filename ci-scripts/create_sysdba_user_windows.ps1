 Stop-Service -Name 'FirebirdServerDefaultInstance' -Force
 cmd /c "echo create user SYSDBA password 'masterkey'; exit; | ""C:\Program Files\Firebird\isql.exe"" -user sysdba employee"
 Start-Service -Name 'FirebirdServerDefaultInstance'
