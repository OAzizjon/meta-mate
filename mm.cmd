@echo off
rem mm from PowerShell or cmd in the project folder: .\mm status   (see tools\mm.py)
if exist "%~dp0.venv\Scripts\python.exe" (
	"%~dp0.venv\Scripts\python.exe" "%~dp0tools\mm.py" %*
) else (
	py -3 "%~dp0tools\mm.py" %*
)
