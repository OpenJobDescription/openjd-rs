@echo off
REM Spawns a child process that runs for a long time, then runs itself for a long time.
REM Used to test process tree termination.
start /b cmd /c "for /L %%i in (0,1,19) do @echo Log from child %%i & ping -n 2 127.0.0.1 >nul"
for /L %%i in (0,1,19) do @echo Log from runner %%i & ping -n 2 127.0.0.1 >nul
