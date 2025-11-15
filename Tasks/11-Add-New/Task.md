1. All paths are relative to the root of this git project
2. Read Resources/design-criteria.md and Resources/putio _ 2.8.14 _ putio _ SwaggerHub.pdf
4. If any files in memory, ensure you have latest versions
5. Using Tasks/00-Initial Project/01-Workplan.md as a template, create a workflow at Tasks/11-Add-New/workplan.md(add phases for each feature that will have human test cli with claude to confirm things are working) for adding the following features
   * download cli command with these properties
        * all of download core code implemented in putio-sync-core and cli related stuff in our cli package. 
        * downloads have a configurable set of workers. default is 2 workers but can be changed in config file or command line options
        * downloads sync to a directory in config. it can be overridden in command line
        * we should have options(default to progress bar) for showing progress. there should be option for no progress. default should be one bar each file transferring with percentage and speed of transfer, then an overall progress bar for all syncs left to do. use file size as progress percentage instead of file count. have alternate views for progress as well like overall only, or just the individual files, or a rotating set of bars between each file, then overall. recognize with no console and suppress progress
        * download command. It should default to download nothing if no arguments given. --all will download all files. you can specify specific files or --filter with glob patterns. 
        * completed downloads should store download info in local sqlite database with filename and size downloaded. if config is set to ignore previously downloaded files, we will skip files that we have downloaded(both name and filesize matched)
        * downloads can be chunked based on a config file or command line option for chunksize. chunks should try to avoid downloading same file and try to download chunks in order, to avoid having to merge chunks at the end. 
        * any intermediate files(files in progress of downloading or chunks) should start with a "." and have a .downloading
        * downloads should happen in order by date unless another option is provided at the command line( --reverse, --by-name, --by-extension)
        * we should support resuming
        * there is a --daemon mode that can list files periodically and then trigger downloads. daemon can be combined with a --filter to have it only download certain file types automatically 
        * provide suggested features in workplan that can be optional as separate phases
        * add config option, with command line switches as well, to enabled deleting of files on putio after download has finished
        * handle situation where file is deleted on putio before download finishing by deleting local file
        * stream downloads to disk with a max of 16mb default buffer per file to avoid large memory use in downloading. the buffer size can be configured in config file or with command line option
        * ensure we sanitize folder and file names to meet local filesystem in a reproducible way
  * a delete command to remove files from putio. It should support filter options, option to delete a folder with recursive, etc, or --all, or all before a date --before "2026-01"
  * a stat command to show total time spent downloading, total files downloaded, total size, total size by extension(normalized case to lowercase)
  * a history command to see files downloaded or removed using putio cli. 
  * a create/add command that can upload a torrent file(s) or magnet link(s) and start a download
     * support a --watch and then a list of folders to watch for torrent files