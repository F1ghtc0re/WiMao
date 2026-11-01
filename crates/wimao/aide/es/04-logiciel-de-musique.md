# Usar los plugins en su programa de música

WiMao coloca los plugins puenteados en tres carpetas de su carpeta personal, una por formato:

- `~/.vst3/yabridge` para los plugins VST3;
- `~/.vst/yabridge` para los plugins VST2;
- `~/.clap/yabridge` para los plugins CLAP.

El signo `~` designa su carpeta personal. Estas carpetas empiezan por un punto: están ocultas. En un selector de archivos, muestre las carpetas ocultas (a menudo con Ctrl+H) o escriba directamente la ruta.

Son simples plugins de Linux: cualquier programa de música de Linux que cargue estos formatos puede usarlos (Bitwig, Ardour, REAPER, Renoise, Carla, Qtractor…). Por ahora, solo Bitwig se ha probado con WiMao.

## Un ajuste que se hace una sola vez

La mayoría de los programas ya exploran `~/.vst3` y `~/.clap`, subcarpetas incluidas: sus plugins VST3 y CLAP suelen encontrarse solos. VST2 no tiene una ubicación estándar: normalmente hay que añadir `~/.vst/yabridge` a mano. En caso de duda, añada las tres carpetas.

### Bitwig

1. Abra Bitwig y después sus ajustes: **Settings → Locations** (Ajustes → Ubicaciones).
2. En las ubicaciones de plugins, añada las tres carpetas anteriores.
3. Haga clic en **Rescan** para que Bitwig vuelva a leer las carpetas.

### Ardour

En las preferencias, en la parte Plugins, añada si hace falta las carpetas VST anteriores y después inicie la búsqueda de plugins desde el gestor de plugins (Plugin Manager).

### Otros programas

REAPER, Renoise…: añada las mismas carpetas en sus ajustes de plugins y vuelva a lanzar la búsqueda de plugins.

Sus plugins de Windows aparecen entonces en el navegador de plugins, junto a los plugins de Linux. Se usan exactamente igual que los demás.

## Después de cada incorporación

No hay nada que repetir en los ajustes. Después de una instalación o de una sesión de gestor, basta con volver a lanzar la búsqueda de plugins en su programa (o reiniciarlo) para ver los plugins nuevos.

## Cerrar su programa de música antes de algunas acciones

Cuando un plugin de Windows está abierto en su programa, Wine trabaja para él. Por eso WiMao se niega a detener Wine mientras un plugin está en ejecución: si no, el programa perdería el plugin en plena sesión. Ciérrelo antes de:

- instalar un plugin o abrir un gestor;
- reparar;
- guardar o restaurar una copia de seguridad;
- volver atrás desde el historial;
- importar un entorno.

Si lo olvida, WiMao muestra «Cierre su software de música antes de continuar: hay un plugin en ejecución». No se modifica nada: cierre su programa y vuelva a empezar.

## Conviene saber

- La primera carga de un plugin de Windows puede tardar unos segundos más que la de un plugin de Linux.
- Un plugin marcado como **Problema** en WiMao a veces funciona en su programa: la prueba de WiMao usa otro programa anfitrión. Pruébelo antes de preocuparse.
- Después de una vuelta atrás, los plugins retirados desaparecen de estas carpetas. Vuelva a lanzar la búsqueda de plugins para que su programa los olvide.

¿Un plugin se niega a abrirse? Vea [Problemas frecuentes](aide:problemes).
