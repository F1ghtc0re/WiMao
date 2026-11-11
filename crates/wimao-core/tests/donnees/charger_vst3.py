"""Charge un plugin VST3 ponté par yabridge, comme le ferait un logiciel de musique.

Utilisé par le test `plugin_ponte_se_charge_depuis_un_flatpak_simule` (flatpak.rs).
Usage : python3 -I charger_vst3.py <chemin du .so ponté>

Affiche `ModuleEntry -> True` si le plugin a pu démarrer (donc que le chainloader a
trouvé yabridge via `$XDG_DATA_HOME/yabridge`), puis `GetPluginFactory` et `ModuleExit`.
"""
import ctypes
import sys

lib = ctypes.CDLL(sys.argv[1])

# ModuleEntry(void* handle) -> bool
lib.ModuleEntry.restype = ctypes.c_bool
lib.ModuleEntry.argtypes = [ctypes.c_void_p]
print("ModuleEntry ->", lib.ModuleEntry(None), flush=True)

# GetPluginFactory() -> pointeur ; ModuleExit() -> bool
lib.GetPluginFactory.restype = ctypes.c_void_p
print("GetPluginFactory ->", bool(lib.GetPluginFactory()), flush=True)
lib.ModuleExit.restype = ctypes.c_bool
print("ModuleExit ->", lib.ModuleExit(), flush=True)
