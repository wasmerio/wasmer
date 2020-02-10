#!/bin/sh

# This install script is intended to download and install the latest available
# release of Wasmer.
# Installer script inspired by:
#  1) https://raw.githubusercontent.com/golang/dep/master/install.sh
#  2) https://sh.rustup.rs
#  3) https://yarnpkg.com/install.sh
#  4) https://raw.githubusercontent.com/brainsik/virtualenv-burrito/master/virtualenv-burrito.sh
#
# It attempts to identify the current platform and an error will be thrown if
# the platform is not supported.
#
# Environment variables:
# - INSTALL_DIRECTORY (optional): defaults to $HOME/.wasmer
# - WASMER_RELEASE_TAG (optional): defaults to fetching the latest release
# - WASMER_OS (optional): use a specific value for OS (mostly for testing)
# - WASMER_ARCH (optional): use a specific value for ARCH (mostly for testing)
#
# You can install using this script:
# $ curl https://raw.githubusercontent.com/wasmerio/wasmer/master/install.sh | sh

set -e

reset="\033[0m"
red="\033[31m"
green="\033[32m"
yellow="\033[33m"
cyan="\033[36m"
white="\033[37m"
bold="\e[1m"
dim="\e[2m"

# Warning: Remove this on the public repo
RELEASES_URL="https://github.com/wasmerio/wasmer/releases"

WASMER_VERBOSE="verbose"
if [ -z "$WASMER_INSTALL_LOG" ]; then
  WASMER_INSTALL_LOG="$WASMER_VERBOSE"
fi

wasmer_download_json() {
    url="$2"

    # echo "Fetching $url.."
    if test -x "$(command -v curl)"; then
        response=$(curl -s -L -w 'HTTPSTATUS:%{http_code}' -H 'Accept: application/json' "$url")
        body=$(echo "$response" | sed -e 's/HTTPSTATUS\:.*//g')
        code=$(echo "$response" | tr -d '\n' | sed -e 's/.*HTTPSTATUS://')
    elif test -x "$(command -v wget)"; then
        temp=$(mktemp)
        body=$(wget -q --header='Accept: application/json' -O - --server-response "$url" 2> "$temp")
        code=$(awk '/^  HTTP/{print $2}' < "$temp" | tail -1)
        rm "$temp"
    else
        printf "$red> Neither curl nor wget was available to perform http requests.$reset\n"
        exit 1
    fi
    if [ "$code" != 200 ]; then
        printf "$red>File download failed with code $code.$reset\n"
        exit 1
    fi

    eval "$1='$body'"
}

wasmer_download_file() {
    url="$1"
    destination="$2"

    # echo "Fetching $url.."
    if test -x "$(command -v curl)"; then
        if [ "$WASMER_INSTALL_LOG" = "$WASMER_VERBOSE" ]; then
          code=$(curl --progress-bar -w '%{http_code}' -L "$url" -o "$destination")
          printf "\033[K\n\033[1A"
        else
          code=$(curl -s -w '%{http_code}' -L "$url" -o "$destination")
        fi
    elif test -x "$(command -v wget)"; then
        if [ "$WASMER_INSTALL_LOG" = "$WASMER_VERBOSE" ]; then
          code=$(wget --show-progress --progress=bar:force:noscroll -q -O "$destination" --server-response "$url" 2>&1 | awk '/^  HTTP/{print $2}' | tail -1)
          printf "\033[K\n\033[1A";
        else
          code=$(wget --quiet -O "$destination" --server-response "$url" 2>&1 | awk '/^  HTTP/{print $2}' | tail -1)
        fi
    else
        printf "$red> Neither curl nor wget was available to perform http requests.$reset\n"
        exit 1
    fi

    if [ "$code" = 404 ]; then
        printf "$red> Your architecture is not yet supported ($OS-$ARCH).$reset\n"
        echo "> Please open an issue on the project if you would like to use wasmer in your project: https://github.com/wasmerio/wasmer"
        exit 1
    elif [ "$code" != 200 ]; then
        printf "$red>File download failed with code $code.$reset\n"
        exit 1
    fi
}


wasmer_detect_profile() {
  if [ -n "${PROFILE}" ] && [ -f "${PROFILE}" ]; then
    echo "${PROFILE}"
    return
  fi

  local DETECTED_PROFILE
  DETECTED_PROFILE=''
  local SHELLTYPE
  SHELLTYPE="$(basename "/$SHELL")"

  if [ "$SHELLTYPE" = "bash" ]; then
    if [ -f "$HOME/.bashrc" ]; then
      DETECTED_PROFILE="$HOME/.bashrc"
    elif [ -f "$HOME/.bash_profile" ]; then
      DETECTED_PROFILE="$HOME/.bash_profile"
    fi
  elif [ "$SHELLTYPE" = "zsh" ]; then
    DETECTED_PROFILE="$HOME/.zshrc"
  elif [ "$SHELLTYPE" = "fish" ]; then
    DETECTED_PROFILE="$HOME/.config/fish/config.fish"
  fi

  if [ -z "$DETECTED_PROFILE" ]; then
    if [ -f "$HOME/.profile" ]; then
      DETECTED_PROFILE="$HOME/.profile"
    elif [ -f "$HOME/.bashrc" ]; then
      DETECTED_PROFILE="$HOME/.bashrc"
    elif [ -f "$HOME/.bash_profile" ]; then
      DETECTED_PROFILE="$HOME/.bash_profile"
    elif [ -f "$HOME/.zshrc" ]; then
      DETECTED_PROFILE="$HOME/.zshrc"
    elif [ -f "$HOME/.config/fish/config.fish" ]; then
      DETECTED_PROFILE="$HOME/.config/fish/config.fish"
    fi
  fi

  if [ ! -z "$DETECTED_PROFILE" ]; then
    echo "$DETECTED_PROFILE"
  fi
}

wasmer_link() {
  printf "$cyan> Adding to bash profile...$reset\n"
  WASMER_PROFILE="$(wasmer_detect_profile)"
  LOAD_STR="\n# Wasmer\nexport WASMER_DIR=\"$INSTALL_DIRECTORY\"\n[ -s \"\$WASMER_DIR/wasmer.sh\" ] && source \"\$WASMER_DIR/wasmer.sh\"\n"
  SOURCE_STR="# Wasmer config\nexport WASMER_DIR=\"$INSTALL_DIRECTORY\"\nexport WASMER_CACHE_DIR=\"\$WASMER_DIR/cache\"\nexport PATH=\"\$WASMER_DIR/bin:\$WASMER_DIR/globals/wapm_packages/.bin:\$PATH\"\n"

  # We create the wasmer.sh file
  printf "$SOURCE_STR" > "$INSTALL_DIRECTORY/wasmer.sh"

  if [ -z "${WASMER_PROFILE-}" ] ; then
    printf "${red}Profile not found. Tried:\n* ${WASMER_PROFILE} (as defined in \$PROFILE)\n* ~/.bashrc\n* ~/.bash_profile\n* ~/.zshrc\n* ~/.profile.\n"
    echo "\nHow to solve this issue?\n* Create one of them and run this script again"
    echo "* Create it (touch ${WASMER_PROFILE}) and run this script again"
    echo "  OR"
    printf "* Append the following lines to the correct file yourself:$reset\n"
    command printf "${SOURCE_STR}"
  else
    if ! grep -q 'wasmer.sh' "$WASMER_PROFILE"; then
      # if [[ $WASMER_PROFILE = *"fish"* ]]; then
      #   command fish -c 'set -U fish_user_paths $fish_user_paths ~/.wasmer/bin'
      # else
      command printf "$LOAD_STR" >> "$WASMER_PROFILE"
      # fi
    fi
    printf "\033[1A$cyan> Adding to bash profile... ✓$reset\n"
    if [ "$WASMER_INSTALL_LOG" = "$WASMER_VERBOSE" ]; then
      printf "${dim}Note: We've added the following to your $WASMER_PROFILE\n"
      echo "If you have a different profile please add the following:"
      printf "$LOAD_STR$reset\n"
    fi

    version=`$INSTALL_DIRECTORY/bin/wasmer --version` || (
      printf "$red> wasmer was installed, but doesn't seem to be working :($reset\n"
      exit 1;
    )

    printf "$green> Successfully installed $version!\n"
    if [ "$WASMER_INSTALL_LOG" = "$WASMER_VERBOSE" ]; then
      printf "${reset}${dim}wasmer & wapm will be available the next time you open the terminal.\n"
      printf "${reset}${dim}If you want to have the commands available now please execute:\n${reset}source $INSTALL_DIRECTORY/wasmer.sh$reset\n"
    fi
  fi
}


# findWasmerBinDirectory() {
#     EFFECTIVE_WASMERPATH=$(wasmer env WASMERPATH)
#     if [ -z "$EFFECTIVE_WASMERPATH" ]; then
#         echo "Installation could not determine your \$WASMERPATH."
#         exit 1
#     fi
#     if [ -z "$WASMERBIN" ]; then
#         WASMERBIN=$(echo "${EFFECTIVE_WASMERPATH%%:*}/bin" | sed s#//*#/#g)
#     fi
#     if [ ! -d "$WASMERBIN" ]; then
#         echo "Installation requires your WASMERBIN directory $WASMERBIN to exist. Please create it."
#         exit 1
#     fi
#     eval "$1='$WASMERBIN'"
# }

initArch() {
    ARCH=$(uname -m)
    if [ -n "$WASMER_ARCH" ]; then
        printf "$cyan> Using WASMER_ARCH ($WASMER_ARCH).$reset\n"
        ARCH="$WASMER_ARCH"
    fi
    # If you modify this list, please also modify scripts/binary-name.sh
    case $ARCH in
        amd64) ARCH="amd64";;
        x86_64) ARCH="amd64";;
        aarch64) ARCH="arm64";;
        # i386) ARCH="386";;
        *) printf "$red> The system architecture (${ARCH}) is not supported by this installation script.$reset\n"; exit 1;;
    esac
    # echo "ARCH = $ARCH"
}

initOS() {
    OS=$(uname | tr '[:upper:]' '[:lower:]')
    if [ -n "$WASMER_OS" ]; then
        printf "$cyan> Using WASMER_OS ($WASMER_OS).$reset\n"
        OS="$WASMER_OS"
    fi
    case "$OS" in
        darwin) OS='darwin';;
        linux) OS='linux';;
        freebsd) OS='freebsd';;
        # mingw*) OS='windows';;
        # msys*) OS='windows';;
        *) printf "$red> The OS (${OS}) is not supported by this installation script.$reset\n"; exit 1;;
    esac
    # echo "OS = $OS"
}


# unset profile
# test -z "$exclude_profile" && modify_profile
# if [ -n "$profile" ]; then
#     if [ -e $HOME/${profile}.pre-wasmer ]; then
#         backup=" The original\nwas saved to ~/$profile.pre-wasmer."
#     fi
# fi

# source $WASMERPATH/startup.sh

wasmer_install() {
  magenta1="${reset}\033[34;1m"
  magenta2=""
  magenta3=""

  if which wasmer >/dev/null; then
    printf "${reset}Updating Wasmer and WAPM$reset\n"
  else
    printf "${reset}Installing Wasmer and WAPM!$reset\n"
    if [ "$WASMER_INSTALL_LOG" = "$WASMER_VERBOSE" ]; then
      printf "
${magenta1}               ww
${magenta1}               wwwww
${magenta1}        ww     wwwwww  w
${magenta1}        wwwww      wwwwwwwww
${magenta1}ww      wwwwww  w     wwwwwww
${magenta1}wwwww      wwwwwwwwww   wwwww
${magenta1}wwwwww  w      wwwwwww  wwwww
${magenta1}wwwwwwwwwwwwww   wwwww  wwwww
${magenta1}wwwwwwwwwwwwwww  wwwww  wwwww
${magenta1}wwwwwwwwwwwwwww  wwwww  wwwww
${magenta1}wwwwwwwwwwwwwww  wwwww  wwwww
${magenta1}wwwwwwwwwwwwwww  wwwww   wwww
${magenta1}wwwwwwwwwwwwwww  wwwww
${magenta1}   wwwwwwwwwwww   wwww
${magenta1}       wwwwwwww
${magenta1}           wwww
${reset}
"
    fi
  fi

#   if [ -d "$INSTALL_DIRECTORY" ]; then
#     if which wasmer; then
#       local latest_url
#       local specified_version
#       local version_type
#       if [ "$1" = '--nightly' ]; then
#         latest_url=https://nightly.wasmerpkg.com/latest-tar-version
#         specified_version=`curl -sS $latest_url`
#         version_type='latest'
#       elif [ "$1" = '--version' ]; then
#         specified_version=$2
#         version_type='specified'
#       elif [ "$1" = '--rc' ]; then
#         latest_url=https://wasmerpkg.com/latest-rc-version
#         specified_version=`curl -sS $latest_url`
#         version_type='rc'
#       else
#         latest_url=https://wasmerpkg.com/latest-version
#         specified_version=`curl -sS $latest_url`
#         version_type='latest'
#       fi
#       wasmer_version=`wasmer -V`
#       wasmer_alt_version=`wasmer --version`

#       if [ "$specified_version" = "$wasmer_version" -o "$specified_version" = "$wasmer_alt_version" ]; then
#         printf "$green> Wasmer is already at the $specified_version version.$reset\n"
#         exit 0
#       else
#       	printf "$yellow> $wasmer_alt_version is already installed, Specified version: $specified_version.$reset\n"
#         rm -rf "$INSTALL_DIRECTORY"
#       fi
#     else
#       printf "$red> $INSTALL_DIRECTORY already exists, possibly from a past Wasmer install.$reset\n"
#       printf "$red> Remove it (rm -rf $INSTALL_DIRECTORY) and run this script again.$reset\n"
#       exit 0
#     fi
#   fi
  wasmer_download # $1 $2
  wasmer_link
  wasmer_reset
}


wasmer_reset() {
  unset -f wasmer_install wasmer_compareversions wasmer_reset wasmer_download_json wasmer_link wasmer_detect_profile wasmer_download_file wasmer_download wasmer_verify_or_quit
}

# Example taken from
# https://stackoverflow.com/questions/4023830/how-to-compare-two-strings-in-dot-separated-version-format-in-bash
# wasmer_compareversions () {
#     if [[ $1 = $2 ]]
#     then
#         echo "="
#         return 0
#     fi
#     local IFS=.
#     local i ver1=($1) ver2=($2)
#     # fill empty fields in ver1 with zeros
#     for ((i=${#ver1[@]}; i<${#ver2[@]}; i++))
#     do
#         ver1[i]=0
#     done
#     for ((i=0; i<${#ver1[@]}; i++))
#     do
#         if [[ -z ${ver2[i]} ]]
#         then
#             # fill empty fields in ver2 with zeros
#             ver2[i]=0
#         fi
#         if ((10#${ver1[i]} > 10#${ver2[i]}))
#         then
#             echo ">"
#             return 0
#         fi
#         if ((10#${ver1[i]} < 10#${ver2[i]}))
#         then
#             echo "<"
#             return 0
#         fi
#     done
#     echo "="
#     return 0
# }

version() {
    echo "$@" | awk -F. '{ printf("%d%03d%03d%03d\n", $1,$2,$3,$4); }';
}

# TODO: Does not support versions with characters in them yet. Won't work for wasmer_compareversions "1.4.5-rc4" "1.4.5-r5"
wasmer_compareversions () {
    WASMER_VERSION=$(version $1)
    WASMER_COMPARE=$(version $2)
    if [ $WASMER_VERSION = $WASMER_COMPARE ]; then
        echo "="
    elif [ $WASMER_VERSION -gt $WASMER_COMPARE ]; then
        echo ">"
    elif [ $WASMER_VERSION -lt $WASMER_COMPARE ]; then
        echo "<"
    fi
}

wasmer_download() {
  # identify platform based on uname output
  initArch
  initOS

  # determine install directory if required
  if [ -z "$INSTALL_DIRECTORY" ]; then
      if [ -z "$WASMER_DIR" ]; then
          # If WASMER_DIR is not present
          INSTALL_DIRECTORY="$HOME/.wasmer"
      else
          # If WASMER_DIR is present
          INSTALL_DIRECTORY="${WASMER_DIR}"
      fi
  fi

  # assemble expected release artifact name
  BINARY="wasmer-${OS}-${ARCH}.tar.gz"

  # add .exe if on windows
  # if [ "$OS" = "windows" ]; then
  #     BINARY="$BINARY.exe"
  # fi

  # if WASMER_RELEASE_TAG was not provided, assume latest
  if [ -z "$WASMER_RELEASE_TAG" ]; then
      printf "$cyan> Getting wasmer releases...$reset\n"
      wasmer_download_json LATEST_RELEASE "$RELEASES_URL/latest"
      WASMER_RELEASE_TAG=$(echo "${LATEST_RELEASE}" | tr -s '\n' ' ' | sed 's/.*"tag_name":"//' | sed 's/".*//' )
      printf "\033[1A$cyan> Getting wasmer releases... ✓$reset\n"
  fi

  if which wasmer >/dev/null; then
    WASMER_VERSION=$(wasmer --version | sed 's/[a-z[:blank:]]//g')
    WASMER_COMPARE=$(wasmer_compareversions $WASMER_VERSION $WASMER_RELEASE_TAG)
    # printf "version: $WASMER_COMPARE\n"
    case $WASMER_COMPARE in
      # WASMER_VERSION = WASMER_RELEASE_TAG
      "=")
        printf "You are already on the latest release of wasmer: ${WASMER_RELEASE_TAG}\n";
        exit 0
        ;;
      # WASMER_VERSION > WASMER_RELEASE_TAG
      ">")
        printf "You are on a more recent version ($WASMER_VERSION) than the published one (${WASMER_RELEASE_TAG})\n";
        exit 0
        ;;
      # WASMER_VERSION < WASMER_RELEASE_TAG (we continue)
      "<")
      ;;
    esac
  fi
  # fetch the real release data to make sure it exists before we attempt a download
  wasmer_download_json RELEASE_DATA "$RELEASES_URL/tag/$WASMER_RELEASE_TAG"

  BINARY_URL="$RELEASES_URL/download/$WASMER_RELEASE_TAG/$BINARY"
  DOWNLOAD_FILE=$(mktemp -t wasmer.XXXXXXXXXX)

  printf "$cyan> Downloading $WASMER_RELEASE_TAG release...$reset\n"
  wasmer_download_file "$BINARY_URL" "$DOWNLOAD_FILE"
  # echo -en "\b\b"
  printf "\033[1A$cyan> Downloading $WASMER_RELEASE_TAG release... ✓$reset\n"
  printf "\033[K\n\033[1A"
  # printf "\033[1A$cyan> Downloaded$reset\033[K\n"
  # echo "Setting executable permissions."

  # windows not supported yet
  # if [ "$OS" = "windows" ]; then
  #     INSTALL_NAME="$INSTALL_NAME.exe"
  # fi

  # echo "Moving executable to $INSTALL_DIRECTORY/$INSTALL_NAME"

  printf "$cyan> Unpacking contents...$reset\n"

  mkdir -p $INSTALL_DIRECTORY
  # Untar the wasmer contents in the install directory
  tar -C $INSTALL_DIRECTORY -zxf $DOWNLOAD_FILE
  printf "\033[1A$cyan> Unpacking contents... ✓$reset\n"
}

wasmer_verify_or_quit() {
  read -p "$1 [y/N] " -n 1 -r
  echo
  if [[ ! $REPLY =~ ^[Yy]$ ]]
  then
    printf "$red> Aborting$reset\n"
    exit 1
  fi
}

# cd ~
wasmer_install # $1 $2
