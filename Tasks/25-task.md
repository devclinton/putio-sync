1. All paths are relative to the root of this git project
2. Read Resources/design-criteria.md and Resources/putio _ 2.8.14 _ putio _ SwaggerHub.pdf
3. Read Tasks/11-Add-New/workplan.md
4. If any files in memory, ensure you have latest versions
5. with Tasks/11-Add-New/workplan.md. as example, make a workplan for the following
  * remove login command
  * rename get-token to login
  * make a files command moving list as subcommand and remove(delete) 
  * move the create command under transfers command
  * support daemon mode for both sync an watch at same time
  * support configuring watch directories through config
  * create a new server option for daemon mode that hosts api on port 15432 with api endpoint for the following
    * putio files
      * listing
      * deleting
      * any other relevant commands
    * local download path files
      * listing
      * deleting
    * have api endpoint for listing folders and files remotely on putio
    * local files in the the sync path
    * transfers
       * listing
       * pausing
       * canceling
       * adding new torrent files or magnet links
    * queue
       * reorder
       * adding to
       * removing
       * blacklisting file
  Make all the api in a separate crate that is used by cli
* ensure daemon with following options have following thread model
  * cli: server --watch --download --daemon
  * thread model:
     * core thread
      - http list thread to watch for new files(every n seconds)
      - transfer list thread to watch for new transfers( every n seconds)
      - download threads
      - http server and subthreads