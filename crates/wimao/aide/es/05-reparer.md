# Reparar

Con el tiempo, el entorno de Wine puede desajustarse: falta un componente, un plugin ya no está conectado con Linux, Wine se queda bloqueado. El botón **Reparar** busca estos fallos y los corrige.

## ¿Cuándo usarlo?

- Un plugin o un gestor se niega a abrirse.
- Un mensaje de error de WiMao propone **Reparar**: basta un clic, solo se corrige el fallo en cuestión.
- Al iniciarse, WiMao indica que algunos plugins aún no están puenteados, por ejemplo si WiMao se cerró durante una sesión de gestor. El mensaje propone **Reparar**.
- En caso de duda, puede probar **Reparar**: sus plugins no se modifican y WiMao normalmente crea antes un punto de restauración al que puede volver desde el historial.

## Cómo hacerlo

1. Cierre su programa de música.
2. Haga clic en **Reparar**, arriba a la derecha (el icono en forma de engranaje).
3. WiMao busca los problemas. Reparar detiene Wine: si hay programas de Windows abiertos, WiMao los nombra y pregunta **Cerrar y reparar**. **Cancelar** abandona toda la reparación, sin modificar nada.
4. WiMao crea un punto de restauración y después corrige cada fallo encontrado.

Al final, un mensaje indica **Reparación terminada** o **No se encontró ningún problema**.

## Lo que WiMao sabe reparar

- plugins instalados pero aún no puenteados hacia Linux;
- Wine bloqueado: sigue en ejecución aunque ya nada lo usa;
- carpetas del entorno (Documentos, Escritorio…) que llevan a sus carpetas personales: los instaladores las llenarían con sus archivos;
- componentes de Windows ausentes o dañados: Wine Mono (para los programas .NET), DXVK (para la imagen) y la biblioteca msvcp140 de Microsoft;
- la carpeta de descargas de Native Access.

Cada reparación también vuelve a aplicar los ajustes de los gestores instalados y reinicia sus servicios.

## ¿Y si la reparación sale mal?

El punto de restauración creado justo antes permite volver atrás: vea [Historial y vuelta atrás](aide:historique). WiMao conserva los tres últimos puntos creados antes de una reparación, aparte de los de las instalaciones.

Si el fallo vuelve, use **Copiar el informe** en el menú principal y adjúntelo a su petición de ayuda. WiMao quita de él su nombre de usuario, sus direcciones de correo electrónico y el nombre de su ordenador. No se envía nada: es usted quien pega el informe. Vea también [Problemas frecuentes](aide:problemes).
