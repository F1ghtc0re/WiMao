# Gestores de editores

Muchos editores entregan sus plugins mediante su propia aplicación: el gestor. Usted inicia sesión con su cuenta y el gestor descarga, instala y activa sus productos. WiMao sabe instalar y abrir estos cuatro gestores:

- **Native Access 2** (Native Instruments: Kontakt, Massive, Guitar Rig…);
- **Arturia Software Center** (Pigments, Analog Lab…);
- **inMusic Software Center** (plugins AIR: Hype, Velvet, Mini Grand…);
- **Plugin Alliance** (Installation Manager).

Aparecen en la sección **Gestores** de la ventana principal.

## Instalar un gestor

Haga clic en **Instalar** junto a su nombre. WiMao lo descarga y lo instala solo. Después, el botón pasa a ser **Abrir**.

Para Native Access, WiMao hace la pregunta **Enlace de inicio de sesión** si otro programa ya gestiona ese enlace: responda **Usar WiMao**. Si no, la vuelta del inicio de sesión desde su navegador no llegaría a Native Access.

## Una sesión de gestor

1. Haga clic en **Abrir**. Si hay otros programas de Windows abiertos, WiMao le pide permiso para cerrarlos.
2. WiMao crea un punto de restauración y abre el gestor. Un aviso indica que está abierto y que WiMao puentea los plugins nuevos.
3. Inicie sesión en su cuenta. Según el gestor, su navegador puede abrirse para el inicio de sesión y después devolverle al gestor.
4. Instale y active sus productos como de costumbre. Mientras tanto, WiMao puentea cada plugin nuevo en cuanto aparece.
5. Cuando termine, cierre el gestor o haga clic en **He terminado** en el aviso.

Si todavía se está ejecutando un instalador, WiMao espera a que termine. Puede elegir **Seguir esperando** o **Detener de todos modos**. Después, un mensaje resume la sesión, por ejemplo «Native Access 2 cerrado: 3 plugins instalados».

Durante una sesión, las demás acciones (instalar, reparar, guardar copia) aparecen en gris: su descripción emergente indica por qué.

## Consejos para cada gestor

- **Native Access**: necesita un servicio de descarga y una carpeta de descargas. WiMao los instala y los configura solo.
- **Arturia Software Center**: un error de WebView2 al iniciarse no tiene consecuencias.
- **inMusic Software Center**: WiMao vuelve a ejecutar sus instaladores en silencio. Si un producto sigue mostrando «Install» después de instalarse, haga clic de nuevo en él o vuelva a abrir el Software Center.
- **Plugin Alliance**: el Installation Manager puede quedarse bloqueado al 98 % en el último producto. Espere a que todos los productos aparezcan como instalados y ciérrelo entonces. Si lo cierra antes, se pierden los paquetes descargados.

¿Un gestor no se abre o el inicio de sesión no vuelve? Vea [Problemas frecuentes](aide:problemes).

## Otras aplicaciones

Algunos instaladores añadidos con **Añadir un instalador** no instalan ningún plugin: dejan el lanzador del editor (por ejemplo BLEASS Plugins Installer). WiMao detecta el nuevo acceso directo del menú Inicio y coloca el lanzador en la sección **Gestores**, debajo de los gestores conocidos. Su fila dice «Aplicación añadida · Instalado».

- **Abrir** inicia una sesión exactamente como con un gestor: punto de restauración, plugins puenteados a medida que aparecen y, al final, **He terminado**.
- El menú «⋯» ofrece **Quitar de la lista**, tras una confirmación. Solo quita la fila: no se desinstala nada.

Al terminar la instalación, WiMao muestra «{nombre} instalado: ábrelo desde la sección Gestores».

Límite: un lanzador sin acceso directo en el menú Inicio no se detecta.

En la línea de comandos: `wimao-cli application lister`, `wimao-cli application ouvrir <nombre>` y `wimao-cli application retirer <nombre>`.
