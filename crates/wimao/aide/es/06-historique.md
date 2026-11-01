# Historial y vuelta atrás

WiMao anota todo lo que hace en un historial: instalaciones, sesiones de gestor, reparaciones, copias de seguridad, restauraciones, importaciones y vueltas atrás. Desde ahí puede volver al estado anterior a una acción.

## Abrir el historial

Haga clic en el botón en forma de reloj, en la barra de título, o pulse Ctrl+H.

Las acciones se agrupan por día: **Hoy**, **Ayer** y después la fecha. Cada línea indica qué se hizo, sobre qué, a qué hora, y el resultado: **Correcto**, **Fallo** o **Cancelado**. Mientras no se haya hecho ninguna acción, la página muestra «Nada por ahora».

## Los puntos de restauración

Antes de cada instalación, cada sesión de gestor y cada reparación, WiMao crea un punto de restauración: una foto de los ajustes del entorno de Wine y de la lista de sus archivos.

WiMao conserva los diez últimos puntos de las instalaciones y sesiones, y los tres últimos puntos de las reparaciones. Los más antiguos se eliminan automáticamente.

## Volver a antes de una acción

1. Cierre su programa de música.
2. En el historial, busque la acción que quiere deshacer y haga clic en **Volver a antes de esto**.
3. WiMao muestra el punto usado, su fecha y el número de archivos que se eliminarán. Confirme.

WiMao vuelve a poner los ajustes del entorno como estaban y elimina los archivos aparecidos desde entonces: por ejemplo, los plugins de una instalación fallida. Los enlaces a plugins que ya no existen también se retiran de las carpetas de plugins. La vuelta atrás aparece a su vez en el historial.

Atención: una vuelta atrás también deshace todo lo que se hizo **después** de ese punto. Los archivos modificados desde entonces no recuperan su contenido anterior: solo los ajustes lo recuperan.

## ¿Por qué el botón falta o está en gris?

- La acción no creó ningún punto de restauración (una copia de seguridad, por ejemplo).
- El punto se eliminó por ser demasiado antiguo.
- Desde entonces se ha restaurado una copia de seguridad: los puntos anteriores ya no corresponden al entorno y no se proponen.
- Volvió a antes de una acción más antigua: los puntos creados después ya no corresponden al entorno y no se proponen.
- Se canceló la importación, o se creó un entorno nuevo: los puntos existentes describen otro entorno y no se proponen.
- Hay otra operación en curso: espere a que termine.

Para devolver todo el entorno a un estado conocido, use mejor una copia de seguridad: vea [Copia de seguridad y restauración](aide:sauvegarde).
