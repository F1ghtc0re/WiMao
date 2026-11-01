# Primeros pasos

WiMao hace funcionar sus plugins de audio de Windows (VST3, VST2, CLAP) en Linux, dentro de su programa de música (DAW): Bitwig, Ardour, REAPER… No tiene que configurar nada a mano: WiMao instala sus plugins, los conecta con Linux y comprueba que se abren.

## ¿Qué es el entorno de Wine?

Wine permite ejecutar programas de Windows en Linux. Para ello, lo guarda todo en una carpeta que imita un ordenador con Windows: una unidad `C:`, sus programas y sus ajustes. WiMao llama a esta carpeta el **entorno de Wine**.

Sus plugins de Windows y las aplicaciones de los editores (Native Access, Arturia Software Center…) se instalan en este entorno. WiMao lo crea, lo mantiene y lo respalda por usted: nunca necesita abrirlo.

Para que su programa de música vea estos plugins, WiMao los «puentea»: crea para cada uno un pequeño plugin de Linux que hace de enlace con el plugin de Windows.

## El primer inicio

En el primer inicio, un asistente le da la bienvenida. Haga clic en **Comenzar** y después:

- si no encuentra ningún entorno, propone **Crear el entorno**;
- si ya usaba un entorno en `~/.wine-mao`, propone **Importar este entorno (recomendado)** o **Crear un entorno nuevo**. Vea [Importar un entorno existente](aide:import).

Cuando el entorno esté listo, elija **Añadir un instalador**, **Abrir un gestor de editor** o **Más tarde**.

## La ventana principal

- La lista de sus plugins, con el estado de cada uno: **Listo**, **Revisar**, **Problema** o **Sin probar**.
- La sección **Gestores**: las aplicaciones de los editores, para instalar o abrir.
- Arriba a la izquierda, **Añadir un instalador**. Arriba a la derecha: el historial, **Reparar**, **Guardar copia** y el menú principal.

El menú principal también permite cambiar el **Idioma** y abrir esta **Guía de uso** (tecla F1).

## Para empezar bien

1. Añada sus plugins: [Añadir un plugin](aide:ajouter-un-plugin) o [Gestores de editores](aide:gestionnaires).
2. Indique a su programa de música una sola vez dónde encontrarlos: [Usar los plugins en su programa de música](aide:logiciel).
3. Haga de vez en cuando una copia de seguridad en un disco externo: [Copia de seguridad y restauración](aide:sauvegarde).
