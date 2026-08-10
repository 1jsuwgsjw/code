@echo off
setlocal EnableExtensions
chcp 65001 >nul
title Codex Agent Test

set "CODEX_EXE=D:\codex\task\artifacts\codex-custom-windows-x64-2f6d6ddc9\codex-custom.exe"
set "TEST_DIR=D:\codex-agent-test"
set "TEST_HOME=D:\codex-agent-test\.codex-home"
set "SOURCE_HOME=%USERPROFILE%\.codex"
set "AGENT_ROOT=%TEST_DIR%\AGENT\agents\query"
set "RESULT_FILE=%AGENT_ROOT%\tasks\current.json"

if /i "%~1"=="--check" goto check
if /i "%~1"=="--verify" goto verify

if not exist "%CODEX_EXE%" (
  echo [ERROR] codex-custom.exe was not found:
  echo %CODEX_EXE%
  pause
  exit /b 1
)

:menu
cls
echo ============================================================
echo Codex Agent isolated test environment
echo Workspace : %TEST_DIR%
echo Test DB   : %TEST_HOME%
echo.
echo Press ENTER to start with the isolated test DB.
echo Type R to back up and reset the isolated DB/cache, then start.
echo Type S to sync current CC Switch config/auth, then start.
echo Type V to verify the latest Stage 2 AGENT result.
echo Type O to open the test folder.
echo Type Q to quit.
echo ============================================================
set "ACTION="
set /p "ACTION=Action: "

if /i "%ACTION%"=="R" goto reset
if /i "%ACTION%"=="S" goto sync
if /i "%ACTION%"=="V" goto verify_menu
if /i "%ACTION%"=="O" goto open_folder
if /i "%ACTION%"=="Q" exit /b 0
goto launch

:prepare_profile
if not exist "%TEST_DIR%" mkdir "%TEST_DIR%"
if not exist "%TEST_HOME%" mkdir "%TEST_HOME%"

if not exist "%TEST_HOME%\config.toml" (
  if exist "%SOURCE_HOME%\config.toml" (
    copy /y "%SOURCE_HOME%\config.toml" "%TEST_HOME%\config.toml" >nul
  ) else (
    echo [ERROR] Missing source config: %SOURCE_HOME%\config.toml
    exit /b 1
  )
)

if not exist "%TEST_HOME%\auth.json" (
  if exist "%SOURCE_HOME%\auth.json" (
    copy /y "%SOURCE_HOME%\auth.json" "%TEST_HOME%\auth.json" >nul
  ) else (
    echo [ERROR] Missing source auth: %SOURCE_HOME%\auth.json
    exit /b 1
  )
)

exit /b 0

:prepare_project
if not exist "%TEST_DIR%" mkdir "%TEST_DIR%"
if not exist "%AGENT_ROOT%\tools" mkdir "%AGENT_ROOT%\tools"
if not exist "%AGENT_ROOT%\memory\facts" mkdir "%AGENT_ROOT%\memory\facts"

where git >nul 2>&1
if not errorlevel 1 (
  if not exist "%TEST_DIR%\.git" git -C "%TEST_DIR%" init -q
)

>"%TEST_DIR%\AGENT\registry.toml" (
  echo schema_version = 1
  echo.
  echo [agents.query]
  echo path = "agents/query/agent.toml"
  echo enabled = true
)

>"%AGENT_ROOT%\agent.toml" (
  echo schema_version = 1
  echo id = "query"
  echo description = "Reads the isolated test workspace and returns bounded evidence."
  echo constraints_file = "constraints.md"
  echo tools = ["tools/shell.x"]
  echo memory_max_items = 16
  echo memory_max_tokens = 2000
)

>"%AGENT_ROOT%\constraints.md" (
  echo # Query AGENT constraints
  echo.
  echo - Work only inside D:\codex-agent-test.
  echo - Use only registered tools.
  echo - Return the fixed nine-field JSON result.
)

>"%AGENT_ROOT%\tools\shell.x" (
  echo schema_version = 1
  echo id = "shell"
  echo description = "Read files in the isolated test workspace."
  echo kind = "native"
  echo tool = "shell_command"
)

>"%AGENT_ROOT%\memory\index.toml" (
  echo schema_version = 1
  echo items = ["memory/facts/stage2.md"]
)

>"%AGENT_ROOT%\memory\facts\stage2.md" echo ACCEPTED_MEMORY_STAGE2_OK
>"%TEST_DIR%\TEST_TARGET.txt" echo PROJECT_AGENT_STAGE2_OK

>"%TEST_DIR%\TEST_PROMPTS.txt" (
  echo STAGE2_AGENT_TEST
  echo.
  echo You must call agent.query exactly once with this task:
  echo Use shell_command to read D:\codex-agent-test\TEST_TARGET.txt. Also use the accepted memory already present in your AGENT context. Return status completed. The result must contain both PROJECT_AGENT_STAGE2_OK and ACCEPTED_MEMORY_STAGE2_OK. Put TEST_TARGET.txt in evidence, add one short memory candidate, add one short improvement proposal, and set error to null.
  echo.
  echo After agent.query returns, print its status, agent_id, task_id, result, evidence, memory_candidates, and improvement_proposals.
)

exit /b 0

:sync_connection
if not exist "%TEST_HOME%" mkdir "%TEST_HOME%"

if exist "%SOURCE_HOME%\config.toml" (
  copy /y "%SOURCE_HOME%\config.toml" "%TEST_HOME%\config.toml" >nul
) else (
  echo [ERROR] Missing source config: %SOURCE_HOME%\config.toml
  exit /b 1
)

if exist "%SOURCE_HOME%\auth.json" (
  copy /y "%SOURCE_HOME%\auth.json" "%TEST_HOME%\auth.json" >nul
) else (
  echo [ERROR] Missing source auth: %SOURCE_HOME%\auth.json
  exit /b 1
)

exit /b 0

:launch
call :prepare_profile
if errorlevel 1 (
  pause
  goto menu
)
call :prepare_project
if errorlevel 1 (
  pause
  goto menu
)

set "CODEX_HOME=%TEST_HOME%"
cd /d "%TEST_DIR%"
cls
echo ============================================================
echo Codex Agent interactive acceptance test
echo Workspace       : %TEST_DIR%
echo Isolated DB home: %CODEX_HOME%
echo.
echo Test function : root agent.query -^> isolated worker -^> fixed JSON -^> disk
echo First message : copy all text from %TEST_DIR%\TEST_PROMPTS.txt
echo Expected text : PROJECT_AGENT_STAGE2_OK and ACCEPTED_MEMORY_STAGE2_OK
echo More prompts  : %TEST_DIR%\TEST_PROMPTS.txt
echo ============================================================
echo.

"%CODEX_EXE%" --no-alt-screen -C "%TEST_DIR%"
set "CODEX_EXIT=%ERRORLEVEL%"

echo.
echo Codex exited with code %CODEX_EXIT%.
call :verify_result
pause
goto menu

:reset
if exist "%TEST_HOME%" (
  if not exist "%TEST_DIR%\db-backups" mkdir "%TEST_DIR%\db-backups"
  set "BACKUP_DIR=%TEST_DIR%\db-backups\codex-home-%RANDOM%-%RANDOM%"
  move "%TEST_HOME%" "%BACKUP_DIR%" >nul
  echo Previous isolated DB was backed up to:
  echo %BACKUP_DIR%
) else (
  echo No isolated DB exists yet.
)
call :sync_connection
if errorlevel 1 (
  pause
  goto menu
)
echo Isolated DB/cache reset complete.
timeout /t 2 >nul
goto launch

:sync
call :sync_connection
if errorlevel 1 (
  pause
  goto menu
)
echo CC Switch config/auth sync complete.
timeout /t 2 >nul
goto launch

:open_folder
if not exist "%TEST_DIR%" mkdir "%TEST_DIR%"
start "" explorer.exe "%TEST_DIR%"
goto menu

:verify_result
if not exist "%RESULT_FILE%" (
  echo [FAIL] No persisted AGENT result yet:
  echo %RESULT_FILE%
  exit /b 2
)

powershell -NoProfile -ExecutionPolicy Bypass -Command "$r = Get-Content -LiteralPath '%RESULT_FILE%' -Raw | ConvertFrom-Json; $r | ConvertTo-Json -Depth 8; $ok = $r.status -eq 'completed' -and $r.agent_id -eq 'query' -and $r.result -match 'PROJECT_AGENT_STAGE2_OK' -and $r.result -match 'ACCEPTED_MEMORY_STAGE2_OK'; if ($ok) { Write-Host '[PASS] Stage 2 AGENT delegation, memory injection, and persistence are working.' -ForegroundColor Green; exit 0 } else { Write-Host '[FAIL] Result exists but the expected status/markers are missing.' -ForegroundColor Red; exit 3 }"
exit /b %ERRORLEVEL%

:verify
call :prepare_project
call :verify_result
exit /b %ERRORLEVEL%

:verify_menu
call :prepare_project
call :verify_result
echo.
pause
goto menu

:check
call :prepare_profile
if errorlevel 1 exit /b 1
call :prepare_project
if errorlevel 1 exit /b 1
echo CHECK_OK
echo CODEX_EXE=%CODEX_EXE%
echo TEST_DIR=%TEST_DIR%
echo CODEX_HOME=%TEST_HOME%
echo CONFIG=%TEST_HOME%\config.toml
echo AUTH=%TEST_HOME%\auth.json
echo REGISTRY=%TEST_DIR%\AGENT\registry.toml
echo PROMPT=%TEST_DIR%\TEST_PROMPTS.txt
exit /b 0
