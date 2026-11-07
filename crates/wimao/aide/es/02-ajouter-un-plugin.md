# Añadir un plugin

¿Ha descargado el instalador de Windows de un plugin (un archivo `.exe` o `.msi`, o un archivo comprimido `.zip` o `.7z`)? WiMao se encarga del resto.

## Iniciar la instalación

1. Haga clic en **Añadir un instalador** (o pulse Ctrl+O) y elija el archivo. También puede arrastrarlo a la ventana.
2. Si todavía hay un programa de Windows abierto (un gestor, por ejemplo), WiMao lo nombra y pregunta **Cerrar y continuar**: la instalación tiene que detener Wine.
3. WiMao crea primero un punto de restauración y después ejecuta el instalador en silencio, sin preguntas.
4. Luego puentea los plugins nuevos hacia Linux y comprueba que cada uno se abre, en una ventana invisible.

Al final, un mensaje resume lo que se ha hecho, por ejemplo «2 plugins instalados». Los plugins aparecen en la lista. Un plugin reinstalado en una versión más reciente lleva la marca **Actualizado** hasta que se cierra la ventana.

Si el plugin debe activarse con su cuenta (Serum 2, por ejemplo), una ventana «Activación de Serum 2» explica qué hacer.

## Cuando la instalación silenciosa no basta

Algunos instaladores se niegan a instalarse en silencio. WiMao lo detecta: no se ha añadido ningún plugin. Entonces propone **Abrir el instalador**. Se abre la ventana habitual del instalador: sígala como en Windows, manteniendo las carpetas propuestas. Cuando se cierra, WiMao puentea y prueba los plugins nuevos.

## El estado de los plugins

- **Listo**: el plugin se carga correctamente en Wine. Según el caso, WiMao vio abrirse su interfaz o no pudo comprobarla: la explicación bajo su nombre lo indica ("Se carga correctamente; ábralo en su software de música para comprobar su interfaz"). En ambos casos se puede usar; ábralo en su programa de música para ver su interfaz.
- **Revisar**: la prueba no pudo concluir, por ejemplo porque el plugin no terminó de cargarse en 5 minutos. Pruébelo en su programa de música.
- **Problema**: el plugin no se carga o falla en Wine. Vea [Problemas frecuentes](aide:problemes).
- **Sin probar**: el plugin aún no se ha probado. Los plugins CLAP aún no se prueban.

Bajo el nombre de cada plugin, una breve explicación acompaña a este estado.

## Conviene saber

- Si un plugin existe en VST3 y en VST2, WiMao solo conserva el VST3: no tendrá duplicados en su programa de música.
- Los plugins de Windows de 32 bits no son compatibles. Casi todos los plugins recientes son de 64 bits.
- Cierre su programa de música antes de instalar: WiMao se niega a detener Wine mientras se usa un plugin.
- ¿Una instalación fallida? Puede volver atrás: vea [Historial y vuelta atrás](aide:historique).
